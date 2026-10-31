//! Rewrite rules management: request/response header and path rewrites.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Json;
use axum::Router;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, Set,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::api::common::{
    load_site_read, load_site_write, non_empty, parse_uuid, Page, Pagination,
};
use crate::api::error::ApiError;
use crate::api::sites::touch_site;
use crate::api::state::AppState;
use crate::auth::AuthUser;
use crate::grpc::notify_config_changed;
use crate::models::rewrite_rules;

/// Valid rewrite directions.
fn is_valid_direction(dir: &str) -> bool {
    matches!(dir, "request" | "response")
}

fn default_direction() -> String {
    "request".to_string()
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    #[serde(flatten)]
    pub pagination: Pagination,
}

#[derive(Debug, Deserialize)]
pub struct CreateRequest {
    pub name: String,
    #[serde(default = "default_direction")]
    pub direction: String,
    #[serde(default)]
    pub condition_expr: Option<String>,
    #[serde(default = "default_operations")]
    pub operations: serde_json::Value,
    #[serde(default)]
    pub priority: i32,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_operations() -> serde_json::Value {
    serde_json::json!([])
}

/// The only operation types the console offers and the data plane documents.
/// The protocol technically carries `set_body`/`set_status_code` too, but
/// they are deliberately unpublished — a rule of that kind fabricates whole
/// response bodies or status codes — so the API refuses them until they are
/// designed end to end instead of letting them silently take effect.
const OPERATION_TYPES: [&str; 8] = [
    "set_header",
    "add_header",
    "remove_header",
    "set_path",
    "regex_replace_path",
    "set_query_param",
    "remove_query_param",
    "replace_body",
];

/// Largest accepted `replace_body` search pattern, in bytes. Matches the
/// response-replace limits so both body-rewriting features behave alike.
const MAX_REPLACE_BODY_PATTERN_LEN: usize = 512;

/// Largest accepted `replace_body` replacement, in bytes.
const MAX_REPLACE_BODY_REPLACEMENT_LEN: usize = 2048;

/// Validates one rule's operations: every entry must carry a whitelisted
/// `type`, and `replace_body` entries honour the pattern/replacement caps
/// with a compilable regular expression when `is_regex` is set.
fn validate_operations(ops: &serde_json::Value, direction: &str) -> Result<(), ApiError> {
    let Some(items) = ops.as_array() else {
        return Err(ApiError::BadRequest(
            "operations must be a JSON array".to_string(),
        ));
    };
    for item in items {
        let Some(obj) = item.as_object() else {
            return Err(ApiError::BadRequest(
                "every operation must be a JSON object".to_string(),
            ));
        };
        let op_type = obj
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if !OPERATION_TYPES.contains(&op_type) {
            return Err(ApiError::BadRequest(format!(
                "unknown operation type '{op_type}'"
            )));
        }
        // Body replacement is applied to the buffered response body only, so
        // a `replace_body` operation on a request rule would silently do
        // nothing — reject the combination instead.
        if op_type == "replace_body" && direction != "response" {
            return Err(ApiError::BadRequest(
                "replace_body operations are only valid on response rules"
                    .to_string(),
            ));
        }
        if op_type != "replace_body" {
            continue;
        }
        let pattern =
            obj.get("name").and_then(|v| v.as_str()).unwrap_or_default();
        if pattern.is_empty() || pattern.len() > MAX_REPLACE_BODY_PATTERN_LEN
        {
            return Err(ApiError::BadRequest(format!(
                "replace_body pattern must be 1-{MAX_REPLACE_BODY_PATTERN_LEN} bytes"
            )));
        }
        let replacement = obj
            .get("value")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if replacement.len() > MAX_REPLACE_BODY_REPLACEMENT_LEN {
            return Err(ApiError::BadRequest(format!(
                "replace_body replacement must be at most \
                 {MAX_REPLACE_BODY_REPLACEMENT_LEN} bytes"
            )));
        }
        if obj.get("is_regex").and_then(|v| v.as_bool()).unwrap_or(false) {
            // The data plane compiles body patterns as `regex::bytes::Regex`
            // (it matches raw body bytes), so the same crate type decides
            // validity here — a text-only `Regex` would wrongly reject
            // bytes-only patterns such as `(?-u:\xFF)`.
            regex::bytes::Regex::new(pattern).map_err(|err| {
                ApiError::BadRequest(format!(
                    "replace_body pattern is not a valid regular expression: \
                     {err}"
                ))
            })?;
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize, Default)]
pub struct UpdateRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub direction: Option<String>,
    #[serde(default)]
    pub condition_expr: Option<String>,
    #[serde(default)]
    pub operations: Option<serde_json::Value>,
    #[serde(default)]
    pub priority: Option<i32>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

/// Routes contributed to `/api/v1`.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/sites/{site_id}/rewrite-rules", get(list).post(create))
        .route(
            "/sites/{site_id}/rewrite-rules/{rule_id}",
            axum::routing::put(update).delete(remove),
        )
}

/// `GET /api/v1/sites/{site_id}/rewrite-rules`
async fn list(
    State(state): State<AppState>,
    current: AuthUser,
    Path(site_id): Path<String>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Page<rewrite_rules::Model>>, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    load_site_read(&state.db, id, &current).await?;
    let pagination = query.pagination.normalise();

    let paginator = rewrite_rules::Entity::find()
        .filter(rewrite_rules::Column::SiteId.eq(id))
        .order_by_asc(rewrite_rules::Column::Priority)
        .order_by_asc(rewrite_rules::Column::Name)
        .paginate(&state.db, pagination.limit());

    let total = paginator.num_items().await?;
    let rows = paginator.fetch_page(pagination.index()).await?;
    Ok(Json(Page::new(rows, total, pagination)))
}

/// `POST /api/v1/sites/{site_id}/rewrite-rules`
async fn create(
    State(state): State<AppState>,
    current: AuthUser,
    Path(site_id): Path<String>,
    Json(payload): Json<CreateRequest>,
) -> Result<Response, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    load_site_write(&state.db, id, &current).await?;

    if payload.name.trim().is_empty() || payload.name.len() > 200 {
        return Err(ApiError::BadRequest(
            "name must be 1-200 characters".to_string(),
        ));
    }
    if !is_valid_direction(&payload.direction) {
        return Err(ApiError::BadRequest(format!(
            "invalid direction '{}'; expected request or response",
            payload.direction
        )));
    }
    if !payload.operations.is_array() {
        return Err(ApiError::BadRequest(
            "operations must be a JSON array".to_string(),
        ));
    }
    validate_operations(&payload.operations, &payload.direction)?;

    let timestamp = chrono::Utc::now();
    let model = rewrite_rules::ActiveModel {
        id: Set(Uuid::new_v4()),
        site_id: Set(id),
        name: Set(payload.name.trim().to_string()),
        direction: Set(payload.direction),
        condition_expr: Set(non_empty(&payload.condition_expr)),
        operations: Set(payload.operations),
        priority: Set(payload.priority),
        enabled: Set(payload.enabled),
        created_at: Set(timestamp),
        updated_at: Set(timestamp),
    }
    .insert(&state.db)
    .await?;

    tracing::info!(site_id = %id, rule_id = %model.id, "rewrite rule created");
    touch_site(&state, id).await?;
    notify_config_changed(&state, id, Some(&current.email)).await;

    Ok((StatusCode::CREATED, Json(model)).into_response())
}

/// `PUT /api/v1/sites/{site_id}/rewrite-rules/{rule_id}`
async fn update(
    State(state): State<AppState>,
    current: AuthUser,
    Path((site_id, rule_id)): Path<(String, String)>,
    Json(payload): Json<UpdateRequest>,
) -> Result<Json<rewrite_rules::Model>, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    let target = parse_uuid(&rule_id, "rewrite rule id")?;
    load_site_write(&state.db, id, &current).await?;

    let row = find(&state, id, target).await?;
    let existing_direction = row.direction.clone();
    let existing_operations = row.operations.clone();
    let mut active: rewrite_rules::ActiveModel = row.into();

    if let Some(name) = non_empty(&payload.name) {
        if name.len() > 200 {
            return Err(ApiError::BadRequest(
                "name must be at most 200 characters".to_string(),
            ));
        }
        active.name = Set(name);
    }
    if let Some(direction) = non_empty(&payload.direction) {
        if !is_valid_direction(&direction) {
            return Err(ApiError::BadRequest(format!(
                "invalid direction '{direction}'"
            )));
        }
        // A direction flip must not strand direction-incompatible operations
        // already stored on the rule (a `replace_body` left on a request rule
        // would silently do nothing). The console clears such operations on a
        // direction change; the API refuses the combination instead.
        if direction != existing_direction && payload.operations.is_none() {
            let incompatible = existing_operations.as_array().is_some_and(|items| {
                items.iter().any(|item| {
                    item.get("type").and_then(|v| v.as_str()) == Some("replace_body")
                })
            });
            if incompatible {
                return Err(ApiError::BadRequest(
                    "cannot switch this rule to a request direction while it \
                     still carries replace_body operations; replace them or \
                     update the operations in the same request"
                        .to_string(),
                ));
            }
        }
        active.direction = Set(direction);
    }
    if let Some(expr) = payload.condition_expr {
        active.condition_expr = Set(non_empty(&Some(expr)));
    }
    if let Some(ops) = payload.operations {
        if !ops.is_array() {
            return Err(ApiError::BadRequest(
                "operations must be a JSON array".to_string(),
            ));
        }
        let effective_direction = non_empty(&payload.direction)
            .unwrap_or_else(|| existing_direction.clone());
        validate_operations(&ops, &effective_direction)?;
        active.operations = Set(ops);
    }
    if let Some(priority) = payload.priority {
        active.priority = Set(priority);
    }
    if let Some(enabled) = payload.enabled {
        active.enabled = Set(enabled);
    }
    active.updated_at = Set(chrono::Utc::now());

    let updated = active.update(&state.db).await?;
    tracing::info!(site_id = %id, rule_id = %target, "rewrite rule updated");
    touch_site(&state, id).await?;
    notify_config_changed(&state, id, Some(&current.email)).await;

    Ok(Json(updated))
}

/// `DELETE /api/v1/sites/{site_id}/rewrite-rules/{rule_id}`
async fn remove(
    State(state): State<AppState>,
    current: AuthUser,
    Path((site_id, rule_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    let target = parse_uuid(&rule_id, "rewrite rule id")?;
    load_site_write(&state.db, id, &current).await?;
    find(&state, id, target).await?;

    rewrite_rules::Entity::delete_by_id(target)
        .exec(&state.db)
        .await?;

    tracing::info!(site_id = %id, rule_id = %target, "rewrite rule deleted");
    touch_site(&state, id).await?;
    notify_config_changed(&state, id, Some(&current.email)).await;

    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn find(
    state: &AppState,
    site_id: Uuid,
    rule_id: Uuid,
) -> Result<rewrite_rules::Model, ApiError> {
    rewrite_rules::Entity::find_by_id(rule_id)
        .filter(rewrite_rules::Column::SiteId.eq(site_id))
        .one(&state.db)
        .await?
        .ok_or_else(|| {
            ApiError::NotFound(format!("rewrite rule {rule_id} not found"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn known_types_pass_and_unpublished_types_are_refused() {
        for op_type in OPERATION_TYPES {
            let ops = json!([{ "type": op_type, "name": "x", "value": "y" }]);
            let direction = if op_type == "replace_body" {
                "response"
            } else {
                "request"
            };
            assert!(
                validate_operations(&ops, direction).is_ok(),
                "{op_type} must be accepted"
            );
        }
        for op_type in ["set_body", "set_status_code", "drop_header", ""] {
            let ops = json!([{ "type": op_type, "name": "x", "value": "y" }]);
            let err = validate_operations(&ops, "request")
                .expect_err("unpublished type must be refused");
            assert!(
                err.to_string().contains("unknown operation type"),
                "{op_type}: {err}"
            );
        }
    }

    #[test]
    fn replace_body_is_rejected_on_request_rules() {
        let ops = json!([{ "type": "replace_body", "name": "a", "value": "b" }]);
        let err = validate_operations(&ops, "request")
            .expect_err("body replacement on a request rule would silently no-op");
        assert!(
            err.to_string().contains("only valid on response rules"),
            "{err}"
        );
        // The same payload passes on a response rule (covered by the cap test).
    }

    #[test]
    fn replace_body_enforces_pattern_and_replacement_caps() {
        let ok = json!([{ "type": "replace_body", "name": "PingWAF", "value": "" }]);
        assert!(validate_operations(&ok, "response").is_ok());

        let long_name = "a".repeat(513);
        let err = validate_operations(
            &json!([
                { "type": "replace_body", "name": long_name, "value": "b" }
            ]),
            "response",
        )
        .expect_err("a 513-byte pattern is over the cap");
        assert!(err.to_string().contains("1-512 bytes"), "{err}");

        let long_value = "b".repeat(2049);
        let err = validate_operations(
            &json!([
                { "type": "replace_body", "name": "a", "value": long_value }
            ]),
            "response",
        )
        .expect_err("a 2049-byte replacement is over the cap");
        assert!(err.to_string().contains("2048"), "{err}");

        let empty = json!([{ "type": "replace_body", "name": "", "value": "b" }]);
        assert!(validate_operations(&empty, "response").is_err());
    }

    #[test]
    fn replace_body_regex_must_compile() {
        let ok = json!([{
            "type": "replace_body",
            "name": "ver\\s+(\\d+)",
            "value": "v$1",
            "is_regex": true
        }]);
        assert!(validate_operations(&ok, "response").is_ok());

        // A bytes-only pattern is legal because the data plane matches raw
        // body bytes with `regex::bytes::Regex`; the API must not reject it.
        let bytes_only = json!([{
            "type": "replace_body",
            "name": "(?-u:\\xFF)",
            "value": "v",
            "is_regex": true
        }]);
        assert!(validate_operations(&bytes_only, "response").is_ok());

        let bad = json!([{
            "type": "replace_body",
            "name": "ver(",
            "value": "v",
            "is_regex": true
        }]);
        let err = validate_operations(&bad, "response")
            .expect_err("a broken regex must be refused");
        assert!(
            err.to_string().contains("not a valid regular expression"),
            "{err}"
        );

        // Broken regex with the flag off stays a literal pattern.
        let literal = json!([{
            "type": "replace_body", "name": "ver(", "value": "v"
        }]);
        assert!(validate_operations(&literal, "response").is_ok());
    }

    #[test]
    fn non_object_entries_are_refused() {
        assert!(validate_operations(&json!(["set_header"]), "request").is_err());
        assert!(validate_operations(&json!([42]), "request").is_err());
    }
}
