//! The HTTP API and static file serving.

use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde_json::json;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use crate::Workspace;
use crate::settings::{self, Context, SourceRecord};

mod accounts_api;
mod items;
mod settings_api;

type AppState = Arc<Workspace>;

pub fn router(workspace: AppState) -> Router {
    let static_dir = workspace.config.server.static_dir.clone();

    let api = Router::new()
        .route(
            "/contexts",
            get(settings_api::list_contexts).post(settings_api::create_context),
        )
        .route(
            "/contexts/{context}",
            put(settings_api::update_context).delete(settings_api::delete_context),
        )
        .route(
            "/contexts/{context}/hidden-kinds",
            put(settings_api::set_hidden_kinds),
        )
        .route("/contexts/{context}/items", get(items::context_items))
        .route("/contexts/{context}/snoozed", get(items::context_snoozed))
        .route("/contexts/{context}/refresh", post(items::refresh_context))
        .route("/source-types", get(settings_api::list_source_types))
        .route(
            "/source-types/{type}/fields/{field}/options",
            post(settings_api::field_options),
        )
        .route("/account-providers", get(accounts_api::list_providers))
        .route("/accounts", get(accounts_api::list_accounts))
        .route("/accounts/{account}", delete(accounts_api::delete_account))
        .route("/accounts/github", post(accounts_api::add_github))
        .route("/accounts/phabricator", post(accounts_api::add_phabricator))
        .route("/accounts/bugzilla", post(accounts_api::add_bugzilla))
        .route("/accounts/slack", post(accounts_api::add_slack))
        .route(
            "/accounts/google/connect",
            get(accounts_api::connect_google),
        )
        .route("/oauth/google/callback", get(accounts_api::google_callback))
        .route(
            "/sources",
            get(settings_api::list_sources).post(settings_api::create_source),
        )
        .route(
            "/sources/{source}",
            put(settings_api::update_source).delete(settings_api::delete_source),
        )
        .route("/sources/{source}/refresh", post(items::refresh_source))
        .route(
            "/items/{source}/{item}/snooze",
            put(items::snooze_item).delete(items::unsnooze_item),
        )
        .route(
            "/items/{source}/{item}/due",
            put(items::set_due).delete(items::clear_due),
        )
        .route(
            "/items/{source}/{item}/actions/{action}",
            post(items::perform_action),
        )
        .route("/activity", post(items::activity))
        .route("/events", get(items::events))
        .fallback(|| async { ApiError::NotFound("not found".to_string()) });

    let router = Router::new().nest("/api", api);
    // An empty `static_dir` serves no frontend, e.g. in development, where
    // Vite serves it.
    let router = if static_dir.as_os_str().is_empty() {
        router
    } else {
        let static_files =
            ServeDir::new(&static_dir).fallback(ServeFile::new(static_dir.join("index.html")));
        router.fallback_service(static_files)
    };
    router
        .layer(TraceLayer::new_for_http())
        .with_state(workspace)
}

#[derive(Debug)]
pub enum ApiError {
    BadRequest(String),
    NotFound(String),
    Conflict(String),
    /// A service the dashboard talks to (e.g. a source's API) failed.
    Upstream(String),
    Internal(String),
}

impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        tracing::error!(%error, "database error");
        ApiError::Internal("database error".to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            ApiError::NotFound(message) => (StatusCode::NOT_FOUND, message),
            ApiError::Conflict(message) => (StatusCode::CONFLICT, message),
            ApiError::Upstream(message) => (StatusCode::BAD_GATEWAY, message),
            ApiError::Internal(message) => (StatusCode::INTERNAL_SERVER_ERROR, message),
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}

type ApiResult<T> = Result<T, ApiError>;

async fn find_context(ws: &Workspace, id: &str) -> ApiResult<Context> {
    settings::get_context(&ws.pool, id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("unknown context `{id}`")))
}

async fn find_source(ws: &Workspace, id: &str) -> ApiResult<SourceRecord> {
    settings::get_source(&ws.pool, id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("unknown source `{id}`")))
}
