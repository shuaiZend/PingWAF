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

//! Fallback error page plugin — answers requests that matched **no site**.
//!
//! The generated `pingwaf_unmatched_host` location (host-less, `path: "/"`,
//! `weight: 0`, no upstream) only exists when the control plane has enabled
//! pages with the `unmatched_host` scope, so this plugin never competes with
//! a real site: it only runs where pingap would otherwise return a bare 404
//! from `http_response.rs`. The plugin short-circuits in the request phase —
//! there is no upstream to contact — and renders the configured Tera page.
//!
//! Template context includes the console's default error-page variables.
//! Host and path are HTML-escaped before entering the context. Site and
//! upstream values are empty because this request matched no site.
//!
//! Page bodies are capped at 256 KiB both at configuration time and after
//! rendering, so a runaway template cannot balloon the response.

use super::{Error, get_hash_key};
use async_trait::async_trait;
use http::header;
use http::{HeaderValue, StatusCode};
use pingap_config::PluginConf;
use pingap_core::{
    Ctx, HttpResponse, Plugin, PluginStep, RequestPluginResult, get_host,
};
use pingora::proxy::Session;
use pingwaf_challenge::generate_request_id;
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use tera::{Context, Tera};
use tracing::{debug, warn};

/// Plugin category name used in error messages and factory registration.
pub const CATEGORY: &str = "fallback_page";
/// Hard ceiling for a single page, applied to the configured template and to
/// the rendered output alike.
pub const MAX_PAGE_BYTES: usize = 256 * 1024;

type Result<T, E = Error> = std::result::Result<T, E>;

fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid {
        category: CATEGORY.to_string(),
        message: message.into(),
    }
}

/// A fallback page from the plugin config (before compilation).
#[derive(Debug, Clone)]
struct FallbackPageConfig {
    status_code: u16,
    content_type: String,
    template: String,
    name: String,
}

/// A compiled fallback page: the template lives in the plugin's [`Tera`]
/// instance under `template_name`.
#[derive(Debug, Clone)]
struct CompiledFallbackPage {
    status_code: u16,
    content_type: String,
    template_name: String,
    name: String,
}

/// Builds the `Content-Type` header value, appending `charset=utf-8` for
/// textual types that did not state one.
fn content_type_value(content_type: &str) -> HeaderValue {
    let mut value = content_type.trim().to_string();
    if value.is_empty() {
        value = "text/html".to_string();
    }
    let lower = value.to_ascii_lowercase();
    if (lower.starts_with("text/") || lower.contains("json"))
        && !lower.contains("charset")
    {
        value.push_str("; charset=utf-8");
    }
    HeaderValue::from_str(&value).unwrap_or_else(|_| {
        HeaderValue::from_static("text/html; charset=utf-8")
    })
}

/// HTML-escapes a value before it enters the Tera context. Kept local so the
/// safety guarantee does not depend on any tera version's helper signature:
/// `&` `<` `>` `"` `'` are replaced with their named entities, which makes an
/// interpolated value inert in element content and in double-quoted
/// attributes alike.
fn escape_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            other => out.push(other),
        }
    }
    out
}

/// Minimal plain-text body used whenever a configured page cannot be served.
fn plain_fallback(status: u16, request_id: &str) -> (String, String) {
    (
        format!("{status} Not Found\nRequest ID: {request_id}\n"),
        "text/plain".to_string(),
    )
}

/// Parses the `pages` array from the plugin config.
fn parse_pages(value: &PluginConf) -> Vec<FallbackPageConfig> {
    let Some(items) = value.get("pages").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            Some(FallbackPageConfig {
                status_code: item
                    .get("status_code")
                    .and_then(|v| v.as_integer())?
                    as u16,
                content_type: item
                    .get("content_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("text/html")
                    .to_string(),
                template: item
                    .get("template")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                name: item
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
            })
        })
        .collect()
}

/// Fallback error page plugin.
pub struct FallbackPagePlugin {
    /// Compiled pages indexed by status code. The control plane enforces
    /// `(scope, status_code)` uniqueness, so at most one page per status.
    pages: Arc<HashMap<u16, CompiledFallbackPage>>,
    /// Status of the page served when the request itself does not select one.
    default_status: u16,
    tera: Arc<Tera>,
    hash_value: String,
}

impl FallbackPagePlugin {
    /// Create a new plugin from configuration.
    pub fn new(params: &PluginConf) -> Result<Self> {
        debug!(params = params.to_string(), "new fallback_page plugin");
        Self::try_from(params)
    }

    /// Renders `page` for the current request. Falls back to a plain-text
    /// body when the template errors or renders past [`MAX_PAGE_BYTES`].
    fn render_page(
        &self,
        page: &CompiledFallbackPage,
        host: &str,
        request_id: &str,
        path: &str,
    ) -> (String, String) {
        // `host` is escaped *before* it enters the context: whatever the
        // template does, the value it interpolates is already inert HTML.
        let mut ctx = Context::new();
        ctx.insert("host", &escape_html(host));
        ctx.insert("request_id", request_id);
        ctx.insert("error_code", &page.status_code);
        ctx.insert(
            "error_message",
            StatusCode::from_u16(page.status_code)
                .ok()
                .and_then(|s| s.canonical_reason())
                .unwrap_or("Unknown Error"),
        );
        ctx.insert("request_host", &escape_html(host));
        ctx.insert("request_path", &escape_html(path));
        ctx.insert("timestamp", &chrono::Utc::now().to_rfc3339());
        for key in ["client_ip", "site_name", "upstream_name", "upstream_addr"]
        {
            ctx.insert(key, "");
        }
        match self.tera.render(&page.template_name, &ctx) {
            Ok(rendered) if rendered.len() <= MAX_PAGE_BYTES => {
                (rendered, page.content_type.clone())
            },
            Ok(_) => {
                warn!(
                    page = %page.name,
                    max = MAX_PAGE_BYTES,
                    "fallback_page rendered past the size cap, using plain text"
                );
                plain_fallback(page.status_code, request_id)
            },
            Err(e) => {
                warn!(
                    page = %page.name,
                    error = %e,
                    "fallback_page render failed, using plain text"
                );
                plain_fallback(page.status_code, request_id)
            },
        }
    }

    /// Builds the final response for an unmatched host.
    fn respond(
        &self,
        status: StatusCode,
        body: String,
        content_type: &str,
    ) -> RequestPluginResult {
        let response = HttpResponse::builder(status)
            .body(body)
            .header((header::CONTENT_TYPE, content_type_value(content_type)))
            .no_store()
            .finish();
        RequestPluginResult::Respond(response)
    }
}

impl TryFrom<&PluginConf> for FallbackPagePlugin {
    type Error = Error;

    fn try_from(value: &PluginConf) -> Result<Self> {
        let hash_value = get_hash_key(value);
        let mut tera = Tera::default();
        let mut map: HashMap<u16, CompiledFallbackPage> = HashMap::new();

        for (i, page) in parse_pages(value).into_iter().enumerate() {
            if page.template.len() > MAX_PAGE_BYTES {
                return Err(invalid(format!(
                    "template for status {} exceeds {} bytes",
                    page.status_code, MAX_PAGE_BYTES
                )));
            }
            if page.template.trim().is_empty() {
                return Err(invalid(format!(
                    "template for status {} must not be empty",
                    page.status_code
                )));
            }
            let template_name =
                format!("pw_fallback_{}_{}", page.status_code, i);
            tera.add_raw_template(&template_name, &page.template)
                .map_err(|e| {
                    invalid(format!(
                        "invalid template for status {} ({}): {e}",
                        page.status_code, page.name
                    ))
                })?;
            map.insert(
                page.status_code,
                CompiledFallbackPage {
                    status_code: page.status_code,
                    content_type: page.content_type,
                    template_name,
                    name: page.name,
                },
            );
        }

        let default_status = map
            .keys()
            .copied()
            .min()
            // The generated config always carries pages; an empty one stays
            // valid so the plugin never panics and simply answers 404.
            .unwrap_or(404);

        Ok(Self {
            pages: Arc::new(map),
            default_status,
            tera: Arc::new(tera),
            hash_value,
        })
    }
}

#[async_trait]
impl Plugin for FallbackPagePlugin {
    #[inline]
    fn config_key(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.hash_value)
    }

    async fn handle_request(
        &self,
        _step: PluginStep,
        session: &mut Session,
        _ctx: &mut Ctx,
    ) -> pingora::Result<RequestPluginResult> {
        let host = get_host(session.req_header())
            .unwrap_or_default()
            .to_string();
        let request_id = generate_request_id();

        // Pages are selected by configuration: the lowest configured status
        // wins. Without any compiled page the plugin behaves like the edge
        // default — a plain, no-store 404.
        let Some(page) = self.pages.get(&self.default_status) else {
            let (body, content_type) = plain_fallback(404, &request_id);
            return Ok(self.respond(
                StatusCode::NOT_FOUND,
                body,
                &content_type,
            ));
        };

        let (body, content_type) = self.render_page(
            page,
            &host,
            &request_id,
            session.req_header().uri.path(),
        );
        let status = StatusCode::from_u16(page.status_code)
            .unwrap_or(StatusCode::NOT_FOUND);
        debug!(
            page = %page.name,
            status = page.status_code,
            host = %host,
            "fallback_page served unmatched host"
        );
        Ok(self.respond(status, body, &content_type))
    }
}

register_plugin!("fallback_page", FallbackPagePlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::h1_session as session;
    use pretty_assertions::assert_eq;

    fn plugin_from(toml: &str) -> FallbackPagePlugin {
        FallbackPagePlugin::new(&toml::from_str::<PluginConf>(toml).unwrap())
            .unwrap()
    }

    const SIMPLE_PAGE: &str = r###"
category = "fallback_page"

[[pages]]
status_code = 404
content_type = "text/html"
name = "Not Found"
template = "<html><body><h1>404</h1><p>{{ host }}</p><p>{{ request_id }}</p></body></html>"
"###;

    #[tokio::test]
    async fn test_serves_page_with_escaped_host() {
        let plugin = plugin_from(SIMPLE_PAGE);
        let mut s = session("GET", "/", "<script>alert(1)</script>").await;
        let result = plugin
            .handle_request(PluginStep::Request, &mut s, &mut Ctx::default())
            .await
            .unwrap();
        let RequestPluginResult::Respond(response) = result else {
            panic!("expected Respond");
        };
        assert_eq!(StatusCode::NOT_FOUND, response.status);
        let body = String::from_utf8(response.body.to_vec()).unwrap();
        assert!(
            body.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
            "host must be HTML-escaped: {body}"
        );
        assert_eq!(
            false,
            body.contains("<script>"),
            "raw host must never be reflected: {body}"
        );
        let headers = response.headers.clone().unwrap();
        assert!(headers.iter().any(|(name, value)| {
            name == http::header::CONTENT_TYPE
                && value.to_str().unwrap().starts_with("text/html")
        }));
    }

    #[tokio::test]
    async fn test_console_default_variables_render_html() {
        let plugin = plugin_from(
            r###"
category = "fallback_page"
[[pages]]
status_code = 404
content_type = "text/html"
template = "<html><h1>{{ error_code }} {{ error_message }}</h1><p>{{ request_path }} on {{ request_host }}</p><p>{{ timestamp }} {{ request_id }}</p>{{ client_ip }}{{ site_name }}{{ upstream_name }}{{ upstream_addr }}</html>"
"###,
        );
        let mut s = session("GET", "/probe", "unknown.example").await;
        let result = plugin
            .handle_request(PluginStep::Request, &mut s, &mut Ctx::default())
            .await
            .unwrap();
        let RequestPluginResult::Respond(response) = result else {
            panic!("expected response");
        };
        let body = String::from_utf8(response.body.to_vec()).unwrap();
        assert!(
            body.starts_with("<html>"),
            "must not fall back to plain text: {body}"
        );
        assert!(body.contains("404 Not Found"));
        assert!(body.contains("/probe on unknown.example"));
        assert!(!body.contains("{{"));
    }

    /// The unmatched-host page interpolates a client-supplied Host header and
    /// request path into an HTML document; both must be escaped. This is the
    /// regression guard for the variables the console default page relies on.
    #[tokio::test]
    async fn test_console_default_variables_are_escaped() {
        let plugin = plugin_from(
            r###"
category = "fallback_page"
[[pages]]
status_code = 404
content_type = "text/html"
template = "<p>{{ request_path }}|{{ request_host }}</p>"
"###,
        );
        let mut s = session("GET", "/a&b", "evil&host<script>").await;
        let result = plugin
            .handle_request(PluginStep::Request, &mut s, &mut Ctx::default())
            .await
            .unwrap();
        let RequestPluginResult::Respond(response) = result else {
            panic!("expected response");
        };
        let body = String::from_utf8(response.body.to_vec()).unwrap();
        assert!(
            body.contains("/a&amp;b|evil&amp;host&lt;script&gt;"),
            "request_path and request_host must be HTML-escaped: {body}"
        );
        assert!(
            !body.contains("<script>") && !body.contains("evil&host"),
            "raw client input must never be reflected: {body}"
        );
    }

    #[tokio::test]
    async fn test_empty_config_answers_plain_404() {
        let plugin = plugin_from(r###"category = "fallback_page""###);
        let mut s = session("GET", "/", "unknown.example.com").await;
        let result = plugin
            .handle_request(PluginStep::Request, &mut s, &mut Ctx::default())
            .await
            .unwrap();
        let RequestPluginResult::Respond(response) = result else {
            panic!("expected Respond");
        };
        assert_eq!(StatusCode::NOT_FOUND, response.status);
        assert!(
            String::from_utf8(response.body.to_vec())
                .unwrap()
                .starts_with("404 Not Found")
        );
        let headers = response.headers.clone().unwrap();
        assert!(headers.iter().any(|(name, value)| {
            name == http::header::CONTENT_TYPE
                && value.to_str().unwrap().starts_with("text/plain")
        }));
    }

    #[test]
    fn test_template_size_cap_rejects_oversized_page() {
        let mut toml = String::from(
            "category = \"fallback_page\"\n\n[[pages]]\nstatus_code = 503\ntemplate = \"",
        );
        toml.push_str(&"x".repeat(MAX_PAGE_BYTES + 1));
        toml.push_str("\"\n");
        let conf = toml::from_str::<PluginConf>(&toml).unwrap();
        assert!(
            FallbackPagePlugin::try_from(&conf).is_err(),
            "a template past 256 KiB must be rejected at init"
        );
    }

    #[test]
    fn test_default_status_is_lowest_page() {
        let plugin = plugin_from(
            r###"
category = "fallback_page"

[[pages]]
status_code = 503
template = "unavailable {{ host }}"

[[pages]]
status_code = 404
template = "not found {{ host }}"
"###,
        );
        assert_eq!(404, plugin.default_status);
        assert_eq!(2, plugin.pages.len());
    }
}
