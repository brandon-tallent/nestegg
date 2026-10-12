//! Authenticated transaction edits: validate first, write Monarch, then mirror its reply.
use crate::{Config, Database, MonarchClient};
use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashSet, sync::Arc};
use tokio::sync::Mutex;

pub struct ApiState {
    pub config: Config,
    pub db: Database,
    pub token: String,
    pub writes: Mutex<()>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditRequest {
    pub ids: Vec<String>,
    pub field: EditField,
    pub value: String,
    #[serde(default)]
    pub dry_run: bool,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditField {
    Merchant,
    Category,
}
#[derive(Serialize)]
pub struct EditResult {
    pub id: String,
    pub updated: bool,
    pub error: Option<String>,
}
#[derive(Serialize)]
pub struct EditResponse {
    pub results: Vec<EditResult>,
    pub dry_run: bool,
}

pub fn router(state: Arc<ApiState>) -> Router {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/v1/transaction-edits", post(edit))
        .layer(middleware::from_fn_with_state(state.clone(), authorize))
        .with_state(state)
}
async fn authorize(
    State(state): State<Arc<ApiState>>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let supplied = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if !token_matches(supplied, &state.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(next.run(request).await)
}
fn token_matches(supplied: &str, expected: &str) -> bool {
    supplied.len() == expected.len()
        && !expected.is_empty()
        && supplied
            .bytes()
            .zip(expected.bytes())
            .fold(0u8, |diff, (a, b)| diff | (a ^ b))
            == 0
}
fn validate(request: &EditRequest) -> Result<(), String> {
    if request.ids.is_empty() || request.ids.len() > 500 {
        return Err("Select between 1 and 500 transactions".into());
    }
    if request.value.trim().is_empty()
        || request.value.len() > 200
        || request.value.chars().any(char::is_control)
    {
        return Err("Enter a name of 1–200 characters".into());
    }
    let mut ids = HashSet::new();
    for id in &request.ids {
        if id.is_empty()
            || id.len() > 128
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
            || !ids.insert(id)
        {
            return Err("Invalid or duplicate transaction id".into());
        }
    }
    Ok(())
}
fn bad(message: impl Into<String>) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, message.into())
}
async fn edit(
    State(state): State<Arc<ApiState>>,
    Json(request): Json<EditRequest>,
) -> Result<Json<EditResponse>, (StatusCode, String)> {
    validate(&request).map_err(bad)?;
    let _guard = state.writes.lock().await;
    if !state
        .db
        .contains_transactions(&request.ids)
        .await
        .map_err(|_| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "Database unavailable".into(),
            )
        })?
    {
        return Err(bad(
            "One or more selected transactions are no longer in the mirror; reload",
        ));
    }
    // Re-read session files per batch so a scheduled browser refresh is respected.
    let client = MonarchClient::new(&state.config).map_err(|_| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "Monarch session unavailable".into(),
        )
    })?;
    let value = request.value.trim();
    let (groups, categories) = client.fetch_categories().await.map_err(|_| {
        (
            StatusCode::BAD_GATEWAY,
            "Could not read Monarch categories".into(),
        )
    })?;
    let category: Option<Value> = if matches!(request.field, EditField::Category) {
        let mut category = categories
            .iter()
            .find(|c| {
                c["name"]
                    .as_str()
                    .is_some_and(|n| n.eq_ignore_ascii_case(value))
            })
            .ok_or_else(|| bad("Category does not exist in Monarch"))?
            .clone();
        if let Some(group) = groups.iter().find(|g| g["id"] == category["group"]["id"]) {
            category["group"] = group.clone();
        }
        if category["id"].as_str().is_none() || category["group"]["type"].as_str().is_none() {
            return Err(bad("Monarch category is missing its id or group"));
        }
        Some(category)
    } else {
        None
    };
    if request.dry_run {
        return Ok(Json(EditResponse {
            results: Vec::new(),
            dry_run: true,
        }));
    }
    let mut results = Vec::new();
    for id in request.ids {
        let merchant = matches!(request.field, EditField::Merchant).then_some(value);
        let category_id = category.as_ref().and_then(|c| c["id"].as_str());
        let result = match client
            .update_transaction_confirmed(&id, merchant, category_id)
            .await
        {
            Ok(mut canonical) => {
                let matched = categories
                    .iter()
                    .find(|c| c["id"] == canonical["category"]["id"]);
                let group =
                    matched.and_then(|c| groups.iter().find(|g| g["id"] == c["group"]["id"]));
                if category
                    .as_ref()
                    .is_some_and(|c| c["id"] != canonical["category"]["id"])
                {
                    results.push(EditResult {
                        id,
                        updated: false,
                        error: Some(
                            "Monarch returned a different category; reload before retrying".into(),
                        ),
                    });
                    continue;
                }
                if let Some(group) = group {
                    canonical["category"]["group"] = group.clone();
                }
                state.db.mirror_transaction_edit(&canonical, group.and_then(|g| g["type"].as_str())).await
                    .map_err(|_| "Monarch updated this transaction, but mirror refresh failed; reload after the next sync".to_string())
            }
            Err(_) => Err("Monarch did not confirm this update; reload before retrying".into()),
        };
        results.push(EditResult {
            id,
            updated: result.is_ok(),
            error: result.err(),
        });
    }
    Ok(Json(EditResponse {
        results,
        dry_run: false,
    }))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> EditRequest {
        EditRequest {
            ids: vec!["00000000-0000-0000-0000-000000000001".into()],
            field: EditField::Merchant,
            value: "New merchant".into(),
            dry_run: false,
        }
    }
    #[test]
    fn invalid_batches_are_rejected_before_any_write() {
        let mut r = request();
        assert!(validate(&r).is_ok());
        r.ids.push(r.ids[0].clone());
        assert!(validate(&r).is_err());
        r.ids.clear();
        assert!(validate(&r).is_err());
        r = request();
        r.value = " \n".into();
        assert!(validate(&r).is_err());
        r = request();
        r.ids[0] = "bad;id".into();
        assert!(validate(&r).is_err());
    }
    #[test]
    fn authentication_requires_the_whole_nonempty_token() {
        assert!(token_matches("secret", "secret"));
        assert!(!token_matches("", ""));
        assert!(!token_matches("secrex", "secret"));
        assert!(!token_matches("secret more", "secret"));
    }
}
