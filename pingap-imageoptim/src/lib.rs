// Copyright 2024-2025 Tree xie.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::optimizer::{
    load_image, optimize_avif, optimize_jpeg, optimize_png, optimize_webp,
};
use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use ctor::ctor;
use pingap_config::PluginConf;
use pingap_core::HTTP_HEADER_TRANSFER_CHUNKED;
use pingap_core::{
    Ctx, Plugin, PluginStep, RequestPluginResult, ResponsePluginResult,
};
use pingap_core::{ModifyResponseBody, ResponseBodyPluginResult};
use pingap_plugin::{
    Error, get_hash_key, get_int_conf, get_plugin_factory, get_str_conf,
};
use pingora::http::ResponseHeader;
use pingora::proxy::Session;
use std::borrow::Cow;
use std::collections::HashSet;
use std::convert::TryFrom;
use std::sync::Arc;
use tracing::debug;

mod optimizer;

const PLUGIN_ID: &str = "_image_optimize_";

/// Appends `Accept` to the response's `Vary` header, without duplicating it.
fn append_vary_accept(response: &mut ResponseHeader) {
    let existing = response
        .headers
        .get(http::header::VARY)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    if existing
        .split(',')
        .any(|part| part.trim().eq_ignore_ascii_case("accept"))
    {
        return;
    }
    let value = if existing.trim().is_empty() {
        "Accept".to_string()
    } else {
        format!("{}, Accept", existing.trim())
    };
    if let Ok(value) = http::HeaderValue::from_str(&value) {
        let _ = response.insert_header(http::header::VARY, value);
    }
}

/// Whether the request's `Accept` header explicitly allows `mime`.
///
/// A substring test is not enough: `Accept: image/webp;q=0` *refuses* WebP, and
/// converting for that client would hand it a format it just said it cannot
/// use. Only an explicit media-range counts — `*/*` keeps the origin bytes,
/// which is the conservative reading and what the cache key assumes.
fn accepts_mime(accept: &str, mime: &str) -> bool {
    accept.split(',').any(|part| {
        let mut pieces = part.split(';');
        let media = pieces.next().unwrap_or_default().trim();
        if !media.eq_ignore_ascii_case(mime) {
            return false;
        }
        for param in pieces {
            let param = param.trim();
            let value = param
                .strip_prefix("q=")
                .or_else(|| param.strip_prefix("Q="));
            if let Some(value) = value {
                // q=0 means "not acceptable"; a malformed q keeps the entry.
                return value
                    .trim()
                    .parse::<f32>()
                    .map(|q| q > 0.0)
                    .unwrap_or(true);
            }
        }
        true
    })
}

/// Images up to this size are buffered for re-encoding; larger ones are
/// passed through (the buffer plus the decoded RGBA image would otherwise be
/// several times the payload).
const MAX_IMAGE_BYTES: usize = 16 * 1024 * 1024;

type Result<T, E = Error> = std::result::Result<T, E>;

struct ImageOptimizer {
    image_type: String,
    png_quality: u8,
    jpeg_quality: u8,
    avif_quality: u8,
    avif_speed: u8,
    webp_quality: u8,
    format_type: String,
    buffer: BytesMut,
    /// Set when the body exceeded the buffer cap: later chunks pass through.
    passthrough: bool,
}

impl ModifyResponseBody for ImageOptimizer {
    fn handle(
        &mut self,
        _session: &Session,
        body: &mut Option<bytes::Bytes>,
        end_of_stream: bool,
    ) -> pingora::Result<()> {
        if let Some(data) = body {
            // The header stage bounded the buffer by the declared length; a
            // lying origin can still exceed it, and holding the bytes would
            // then be unbounded. Stop buffering and hand the original through.
            if self.buffer.len() + data.len() > MAX_IMAGE_BYTES {
                let mut original = std::mem::take(&mut self.buffer).to_vec();
                original.extend_from_slice(&data[..]);
                *body = Some(Bytes::from(original));
                self.buffer = BytesMut::new();
                self.passthrough = true;
                return Ok(());
            }
            self.buffer.extend(&data[..]);
            data.clear();
        }
        if self.passthrough || !end_of_stream {
            return Ok(());
        }
        // Decode and re-encode; on any failure send the bytes the origin
        // produced instead of an empty body (the content type was already
        // rewritten at the header stage, so the original bytes are the best
        // remaining answer for a corrupt image).
        let original = std::mem::take(&mut self.buffer);
        let encoded =
            load_image(&original, &self.image_type).and_then(|info| match self
                .format_type
                .as_str()
            {
                "jpeg" => optimize_jpeg(&info, self.jpeg_quality),
                "avif" => {
                    optimize_avif(&info, self.avif_quality, self.avif_speed)
                },
                "webp" => optimize_webp(&info, self.webp_quality),
                _ => optimize_png(&info, self.png_quality),
            });
        match encoded {
            Ok(data) => *body = Some(Bytes::from(data)),
            Err(err) => {
                debug!(
                    target: PLUGIN_ID,
                    error = %err,
                    "image re-encode failed; passing the original bytes through"
                );
                *body = Some(original.freeze());
            },
        }
        Ok(())
    }
    fn name(&self) -> &str {
        "image_optimization"
    }
}

pub struct ImageOptim {
    /// A unique identifier for this plugin instance.
    /// Used for internal tracking and debugging purposes.
    hash_value: String,
    support_types: HashSet<String>,
    output_mimes: Vec<String>,
    png_quality: u8,
    jpeg_quality: u8,
    avif_quality: u8,
    avif_speed: u8,
}

impl TryFrom<&PluginConf> for ImageOptim {
    type Error = Error;
    fn try_from(value: &PluginConf) -> Result<Self> {
        debug!(params = value.to_string(), "new image optimizer plugin");
        let hash_value = get_hash_key(value);

        let output_types: Vec<String> = get_str_conf(value, "output_types")
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();

        let output_mimes = output_types
            .iter()
            .map(|format| format!("image/{}", format))
            .collect();

        let mut png_quality = get_int_conf(value, "png_quality") as u8;
        if png_quality == 0 || png_quality > 100 {
            png_quality = 90;
        }
        let mut jpeg_quality = get_int_conf(value, "jpeg_quality") as u8;
        if jpeg_quality == 0 || jpeg_quality > 100 {
            jpeg_quality = 80;
        }
        let mut avif_quality = get_int_conf(value, "avif_quality") as u8;
        if avif_quality == 0 || avif_quality > 100 {
            avif_quality = 75;
        }
        let mut avif_speed = get_int_conf(value, "avif_speed") as u8;
        if avif_speed == 0 || avif_speed > 10 {
            avif_speed = 3;
        }
        Ok(Self {
            hash_value,
            support_types: HashSet::from([
                "jpeg".to_string(),
                "png".to_string(),
            ]),
            output_mimes,
            png_quality,
            jpeg_quality,
            avif_quality,
            avif_speed,
        })
    }
}

impl ImageOptim {
    pub fn new(params: &PluginConf) -> Result<Self> {
        Self::try_from(params)
    }
}

#[async_trait]
impl Plugin for ImageOptim {
    /// Returns a unique identifier for this plugin instance
    fn config_key(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.hash_value)
    }
    async fn handle_request(
        &self,
        step: PluginStep,
        session: &mut Session,
        ctx: &mut Ctx,
    ) -> pingora::Result<RequestPluginResult> {
        if step != PluginStep::Request {
            return Ok(RequestPluginResult::Skipped);
        }

        if let Some(accept) = session.get_header(http::header::ACCEPT)
            && let Ok(accept_str) = accept.to_str()
        {
            let mut accept_images: Vec<_> = self
                .output_mimes
                .iter()
                .filter(|mime| accepts_mime(accept_str, mime))
                .cloned()
                .collect();

            if !accept_images.is_empty() {
                accept_images.sort();
                ctx.extend_cache_keys(accept_images);
            }
        }
        Ok(RequestPluginResult::Continue)
    }
    fn handle_upstream_response(
        &self,
        session: &mut Session,
        ctx: &mut Ctx,
        upstream_response: &mut ResponseHeader,
    ) -> pingora::Result<ResponsePluginResult> {
        // Only a plain 200 image response is rewritten: a 304 carries no body,
        // a HEAD has no body to convert, and a 206 range would be broken by
        // re-encoding the slice.
        if upstream_response.status != http::StatusCode::OK {
            return Ok(ResponsePluginResult::Unchanged);
        }
        if session.req_header().method == http::Method::HEAD {
            return Ok(ResponsePluginResult::Unchanged);
        }
        // Owned so the header can be mutated below: a `&str` borrowed from
        // the response would keep the whole header borrowed.
        let content_type = match upstream_response
            .headers
            .get(http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
        {
            Some(value) => value.to_string(),
            None => return Ok(ResponsePluginResult::Unchanged),
        };

        let Some(image_type) = content_type.strip_prefix("image/") else {
            return Ok(ResponsePluginResult::Unchanged);
        };
        let image_type = image_type.to_string();

        if !self.support_types.contains(&image_type) {
            return Ok(ResponsePluginResult::Unchanged);
        }

        let Some(accept) = session.get_header(http::header::ACCEPT) else {
            return Ok(ResponsePluginResult::Unchanged);
        };
        let Ok(accept_str) = accept.to_str() else {
            return Ok(ResponsePluginResult::Unchanged);
        };

        // No configured output format is acceptable to this client: leave the
        // response completely alone. Rewriting the content type (or
        // re-encoding to the source format) here was a defect: it turned a
        // pass-through into a lossy re-encode announced as an invalid type.
        let Some(format_type) = self
            .output_mimes
            .iter()
            .find(|mime| accepts_mime(accept_str, mime))
            .cloned()
        else {
            return Ok(ResponsePluginResult::Unchanged);
        };

        // A chunked image has no declared size, so the buffering cost is
        // unbounded; pass it through instead of holding it in memory. The
        // declared length is also what bounds the buffer below.
        let Some(size) = upstream_response
            .headers
            .get(http::header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<usize>().ok())
        else {
            debug!(
                target: PLUGIN_ID,
                "image response without Content-Length; passing through"
            );
            return Ok(ResponsePluginResult::Unchanged);
        };
        // Oversized images are passed through untouched: buffering them would
        // cost the full body plus an RGBA decode (~4x pixels), and the
        // declared length is attacker-controlled (2026-10-18 audit).
        if size > MAX_IMAGE_BYTES {
            debug!(
                target: PLUGIN_ID,
                size, "image exceeds the optimization cap; passing through"
            );
            return Ok(ResponsePluginResult::Unchanged);
        }

        // Remove content-length since the body is about to be replaced.
        let _ = upstream_response.remove_header(&http::header::CONTENT_LENGTH);
        // Switch to chunked transfer encoding
        let _ = upstream_response.insert_header(
            http::header::TRANSFER_ENCODING,
            HTTP_HEADER_TRANSFER_CHUNKED.1.clone(),
        );
        let _ = upstream_response
            .insert_header(http::header::CONTENT_TYPE, &format_type);
        // The body now depends on the request's Accept header, so a fronting
        // shared cache must key on it too; our own cache extends its key in
        // `handle_request`, a CDN in front of us does not.
        append_vary_accept(upstream_response);

        ctx.add_modify_body_handler(
            PLUGIN_ID,
            Box::new(ImageOptimizer {
                image_type: image_type.clone(),
                png_quality: self.png_quality,
                jpeg_quality: self.jpeg_quality,
                avif_quality: self.avif_quality,
                avif_speed: self.avif_speed,
                // only support lossless
                webp_quality: 100,
                format_type,
                buffer: BytesMut::with_capacity(size.min(MAX_IMAGE_BYTES)),
                passthrough: false,
            }),
        );
        Ok(ResponsePluginResult::Modified)
    }
    fn handle_upstream_response_body(
        &self,
        session: &mut Session,
        ctx: &mut Ctx,
        body: &mut Option<bytes::Bytes>,
        end_of_stream: bool,
    ) -> pingora::Result<ResponseBodyPluginResult> {
        if let Some(modifier) = ctx.get_modify_body_handler(PLUGIN_ID) {
            modifier.handle(session, body, end_of_stream)?;
            let result = if end_of_stream {
                ResponseBodyPluginResult::FullyReplaced
            } else {
                ResponseBodyPluginResult::PartialReplaced
            };
            Ok(result)
        } else {
            Ok(ResponseBodyPluginResult::Unchanged)
        }
    }
}

#[ctor(unsafe)]
fn init() {
    get_plugin_factory().register("image_optim", |params| {
        Ok(Arc::new(ImageOptim::new(params)?))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use pingap_config::PluginConf;
    use pingap_core::{Ctx, Plugin};
    use pingora::modules::http::HttpModules;
    use pretty_assertions::assert_eq;
    use tokio_test::io::Builder;

    #[test]
    fn accept_matching_respects_q_values_and_explicit_ranges() {
        assert!(accepts_mime("image/webp", "image/webp"));
        assert!(accepts_mime("image/avif, image/webp", "image/webp"));
        assert!(accepts_mime("IMAGE/WEBP", "image/webp"));
        // q=0 is a refusal, not a preference.
        assert!(!accepts_mime("image/webp;q=0", "image/webp"));
        assert!(!accepts_mime("image/webp;q=0.0", "image/webp"));
        assert!(accepts_mime("image/webp;q=0.5", "image/webp"));
        // A wildcard keeps the origin bytes: only an explicit range converts.
        assert!(!accepts_mime("*/*", "image/webp"));
        assert!(!accepts_mime("image/*", "image/webp"));
        // A longer type must not match by prefix.
        assert!(!accepts_mime("image/webpx", "image/webp"));
        assert!(!accepts_mime("", "image/webp"));
    }

    #[test]
    fn test_new_image_optimize() {
        let optim = ImageOptim::try_from(
            &toml::from_str::<PluginConf>(
                r###"
avif_quality = 75
avif_speed = 3
category = "image_optim"
jpeg_quality = 80
output_types = "avif,webp"
png_quality = 90
"###,
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(
            HashSet::from(["jpeg".to_string(), "png".to_string(),]),
            optim.support_types
        );
        assert_eq!(
            vec!["image/avif".to_string(), "image/webp".to_string()],
            optim.output_mimes
        );
        assert_eq!(90, optim.png_quality);
        assert_eq!(80, optim.jpeg_quality);
        assert_eq!(75, optim.avif_quality);
        assert_eq!(3, optim.avif_speed);
    }

    #[tokio::test]
    async fn test_image_optimize_handle_request() {
        let optim = ImageOptim::try_from(
            &toml::from_str::<PluginConf>(
                r###"
avif_quality = 75
avif_speed = 3
category = "image_optim"
jpeg_quality = 80
output_types = "avif,webp"
png_quality = 90
"###,
            )
            .unwrap(),
        )
        .unwrap();
        // not accept value
        {
            let headers = [""].join("\r\n");
            let input_header = format!(
                "GET /vicanso/pingap?size=1 HTTP/1.1\r\n{headers}\r\n\r\n"
            );
            let mock_io = Builder::new().read(input_header.as_bytes()).build();
            let mut session = Session::new_h1_with_modules(
                Box::new(mock_io),
                &HttpModules::new(),
            );
            session.read_request().await.unwrap();
            let mut ctx = Ctx::default();

            let result = optim
                .handle_request(PluginStep::Request, &mut session, &mut ctx)
                .await
                .unwrap();

            assert_eq!(true, RequestPluginResult::Continue == result);
            assert_eq!(true, ctx.cache.is_none());
        }

        // accept avif
        {
            let headers = ["Accept: image/avif"].join("\r\n");
            let input_header = format!(
                "GET /vicanso/pingap?size=1 HTTP/1.1\r\n{headers}\r\n\r\n"
            );
            let mock_io = Builder::new().read(input_header.as_bytes()).build();
            let mut session = Session::new_h1_with_modules(
                Box::new(mock_io),
                &HttpModules::new(),
            );
            session.read_request().await.unwrap();
            let mut ctx = Ctx::default();

            let result = optim
                .handle_request(PluginStep::Request, &mut session, &mut ctx)
                .await
                .unwrap();

            assert_eq!(true, RequestPluginResult::Continue == result);
            assert_eq!(
                vec!["image/avif".to_string()],
                ctx.cache.unwrap().keys.unwrap()
            );
        }

        // accept avif, webp
        {
            let headers = ["Accept: image/webp, image/avif"].join("\r\n");
            let input_header = format!(
                "GET /vicanso/pingap?size=1 HTTP/1.1\r\n{headers}\r\n\r\n"
            );
            let mock_io = Builder::new().read(input_header.as_bytes()).build();
            let mut session = Session::new_h1_with_modules(
                Box::new(mock_io),
                &HttpModules::new(),
            );
            session.read_request().await.unwrap();
            let mut ctx = Ctx::default();

            let result = optim
                .handle_request(PluginStep::Request, &mut session, &mut ctx)
                .await
                .unwrap();

            assert_eq!(true, RequestPluginResult::Continue == result);
            assert_eq!(
                vec!["image/avif".to_string(), "image/webp".to_string()],
                ctx.cache.unwrap().keys.unwrap()
            );
        }
    }

    #[tokio::test]
    async fn test_image_optimize_handle_upstream_response() {
        let optim = ImageOptim::try_from(
            &toml::from_str::<PluginConf>(
                r###"
avif_quality = 75
avif_speed = 3
category = "image_optim"
jpeg_quality = 80
output_types = "avif,webp"
png_quality = 90
"###,
            )
            .unwrap(),
        )
        .unwrap();

        let headers = ["Accept: image/webp, image/avif"].join("\r\n");
        let input_header =
            format!("GET /vicanso/pingap?size=1 HTTP/1.1\r\n{headers}\r\n\r\n");
        let mock_io = Builder::new().read(input_header.as_bytes()).build();
        let mut session = Session::new_h1_with_modules(
            Box::new(mock_io),
            &HttpModules::new(),
        );
        session.read_request().await.unwrap();
        let mut ctx = Ctx::default();
        let mut upstream_response = ResponseHeader::build(200, None).unwrap();

        // no content type
        let result = optim
            .handle_upstream_response(
                &mut session,
                &mut ctx,
                &mut upstream_response,
            )
            .unwrap();
        assert_eq!(true, ResponsePluginResult::Unchanged == result);

        // content type is not image
        upstream_response
            .append_header("content-type", "application/json")
            .unwrap();
        let result = optim
            .handle_upstream_response(
                &mut session,
                &mut ctx,
                &mut upstream_response,
            )
            .unwrap();
        assert_eq!(true, ResponsePluginResult::Unchanged == result);

        // An image without a declared length is passed through: buffering a
        // chunked body has no bound.
        upstream_response
            .insert_header("content-type", "image/png")
            .unwrap();
        let result = optim
            .handle_upstream_response(
                &mut session,
                &mut ctx,
                &mut upstream_response,
            )
            .unwrap();
        assert_eq!(true, ResponsePluginResult::Unchanged == result);

        // response image png with a declared length: converted, chunked, and
        // the response now varies on Accept.
        upstream_response
            .insert_header("content-length", "1024")
            .unwrap();
        let result = optim
            .handle_upstream_response(
                &mut session,
                &mut ctx,
                &mut upstream_response,
            )
            .unwrap();
        assert_eq!(
            "chunked",
            upstream_response.headers.get("transfer-encoding").unwrap()
        );
        assert_eq!(
            "image/avif",
            upstream_response.headers.get("content-type").unwrap()
        );
        assert_eq!(true, ResponsePluginResult::Modified == result);
        assert_eq!(true, ctx.get_modify_body_handler(PLUGIN_ID).is_some());
        assert_eq!("Accept", upstream_response.headers.get("vary").unwrap());
    }

    #[tokio::test]
    async fn test_accept_without_a_supported_format_is_passed_through() {
        let optim = ImageOptim::try_from(
            &toml::from_str::<PluginConf>(
                r###"
category = "image_optim"
output_types = "avif,webp"
"###,
            )
            .unwrap(),
        )
        .unwrap();

        // `*/*` (fetch, curl, requests) accepts the origin type, not a
        // conversion: the response must stay untouched — no content-type
        // rewrite, no lossy re-encode.
        for accept in ["*/*", "image/png", "image/*", "image/webp;q=0"] {
            let headers = [format!("Accept: {accept}")].join("\r\n");
            let input_header = format!(
                "GET /vicanso/pingap?size=1 HTTP/1.1\r\n{headers}\r\n\r\n"
            );
            let mock_io = Builder::new().read(input_header.as_bytes()).build();
            let mut session = Session::new_h1_with_modules(
                Box::new(mock_io),
                &HttpModules::new(),
            );
            session.read_request().await.unwrap();
            let mut ctx = Ctx::default();
            let mut upstream_response =
                ResponseHeader::build(200, None).unwrap();
            upstream_response
                .insert_header("content-type", "image/png")
                .unwrap();
            upstream_response
                .insert_header("content-length", "1024")
                .unwrap();
            let result = optim
                .handle_upstream_response(
                    &mut session,
                    &mut ctx,
                    &mut upstream_response,
                )
                .unwrap();
            assert_eq!(
                true,
                ResponsePluginResult::Unchanged == result,
                "Accept: {accept} must not trigger a conversion"
            );
            assert_eq!(
                "image/png",
                upstream_response.headers.get("content-type").unwrap(),
                "Accept: {accept} must keep the origin content type"
            );
            assert!(
                ctx.get_modify_body_handler(PLUGIN_ID).is_none(),
                "Accept: {accept} must not register a body rewrite"
            );
        }

        // A 304 carries no body: rewriting its content type would announce a
        // format that is not there.
        let headers = ["Accept: image/webp"].join("\r\n");
        let input_header =
            format!("GET /vicanso/pingap?size=1 HTTP/1.1\r\n{headers}\r\n\r\n");
        let mock_io = Builder::new().read(input_header.as_bytes()).build();
        let mut session = Session::new_h1_with_modules(
            Box::new(mock_io),
            &HttpModules::new(),
        );
        session.read_request().await.unwrap();
        let mut ctx = Ctx::default();
        let mut not_modified = ResponseHeader::build(304, None).unwrap();
        not_modified
            .insert_header("content-type", "image/png")
            .unwrap();
        let result = optim
            .handle_upstream_response(&mut session, &mut ctx, &mut not_modified)
            .unwrap();
        assert_eq!(true, ResponsePluginResult::Unchanged == result);
    }

    #[tokio::test]
    async fn test_a_failed_reencode_keeps_the_original_bytes() {
        let optim = ImageOptim::try_from(
            &toml::from_str::<PluginConf>(
                r###"
category = "image_optim"
output_types = "webp"
"###,
            )
            .unwrap(),
        )
        .unwrap();

        let headers = ["Accept: image/webp"].join("\r\n");
        let input_header =
            format!("GET /vicanso/pingap?size=1 HTTP/1.1\r\n{headers}\r\n\r\n");
        let mock_io = Builder::new().read(input_header.as_bytes()).build();
        let mut session = Session::new_h1_with_modules(
            Box::new(mock_io),
            &HttpModules::new(),
        );
        session.read_request().await.unwrap();
        let mut ctx = Ctx::default();
        let mut upstream_response = ResponseHeader::build(200, None).unwrap();
        upstream_response
            .insert_header("content-type", "image/png")
            .unwrap();
        upstream_response
            .insert_header("content-length", "16")
            .unwrap();
        let result = optim
            .handle_upstream_response(
                &mut session,
                &mut ctx,
                &mut upstream_response,
            )
            .unwrap();
        assert_eq!(true, ResponsePluginResult::Modified == result);

        // Not a PNG: the decode fails. The body must be the original bytes,
        // never an empty one (a 200 with an empty image is worse than an
        // unoptimized one).
        let original = bytes::Bytes::from_static(b"not-really-a-png");
        let mut body = Some(original.clone());
        optim
            .handle_upstream_response_body(
                &mut session,
                &mut ctx,
                &mut body,
                true,
            )
            .unwrap();
        assert_eq!(Some(original), body);
    }
}
