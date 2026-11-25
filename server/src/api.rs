//! The HTTP API and static file serving.

use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio_stream::wrappers::BroadcastStream;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use crate::config::ContextConfig;
use crate::db::{self, Filter, Item, SourceStatus};
use crate::events::Event;
use crate::{SourceEntry, Workspace};

type AppState = Arc<Workspace>;

pub fn router(workspace: AppState) -> Router {
    let static_dir = &workspace.config.server.static_dir;
    let static_files =
        ServeDir::new(static_dir).fallback(ServeFile::new(static_dir.join("index.html")));

    let api = Router::new()
        .route("/contexts", get(list_contexts))
        .route("/contexts/{context}/items", get(context_items))
        .route("/contexts/{context}/snoozed", get(context_snoozed))
        .route("/contexts/{context}/refresh", post(refresh_context))
        .route("/sources", get(list_sources))
        .route("/sources/{source}/refresh", post(refresh_source))
        .route(
            "/items/{source}/{item}/snooze",
            put(snooze_item).delete(unsnooze_item),
        )
        .route("/items/{source}/{item}/due", put(set_due).delete(clear_due))
        .route("/activity", post(activity))
        .route("/events", get(events))
        .fallback(|| async { ApiError::NotFound("not found".to_string()) });

    Router::new()
        .nest("/api", api)
        .fallback_service(static_files)
        .layer(TraceLayer::new_for_http())
        .with_state(workspace)
}

#[derive(Debug)]
pub enum ApiError {
    NotFound(String),
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
            ApiError::NotFound(message) => (StatusCode::NOT_FOUND, message),
            ApiError::Internal(message) => (StatusCode::INTERNAL_SERVER_ERROR, message),
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}

type ApiResult<T> = Result<T, ApiError>;

fn find_context<'a>(ws: &'a Workspace, id: &str) -> ApiResult<&'a ContextConfig> {
    ws.config
        .context(id)
        .ok_or_else(|| ApiError::NotFound(format!("unknown context `{id}`")))
}

fn find_source<'a>(ws: &'a Workspace, id: &str) -> ApiResult<&'a SourceEntry> {
    ws.source(id)
        .ok_or_else(|| ApiError::NotFound(format!("unknown source `{id}`")))
}

async fn find_item(ws: &Workspace, source_id: &str, item_id: &str) -> ApiResult<()> {
    find_source(ws, source_id)?;
    if db::item_exists(&ws.pool, source_id, item_id).await? {
        Ok(())
    } else {
        Err(ApiError::NotFound(format!("unknown item `{item_id}`")))
    }
}

async fn list_contexts(State(ws): State<AppState>) -> Json<Vec<ContextConfig>> {
    Json(ws.config.contexts.clone())
}

#[derive(Serialize)]
struct SourceInfo {
    id: String,
    name: String,
    icon: String,
    contexts: Vec<String>,
    status: SourceStatus,
}

async fn list_sources(State(ws): State<AppState>) -> ApiResult<Json<Vec<SourceInfo>>> {
    let mut statuses = db::source_statuses(&ws.pool).await?;
    Ok(Json(
        ws.sources
            .iter()
            .map(|s| SourceInfo {
                id: s.common.id.clone(),
                name: s.common.name.clone(),
                icon: s.icon.clone(),
                contexts: s.common.contexts.clone(),
                status: statuses.remove(&s.common.id).unwrap_or_default(),
            })
            .collect(),
    ))
}

async fn items(ws: &Workspace, context: &str, filter: Filter) -> ApiResult<Json<Vec<Item>>> {
    find_context(ws, context)?;
    let source_ids: Vec<&str> = ws
        .context_sources(context)
        .map(|s| s.common.id.as_str())
        .collect();
    Ok(Json(
        db::list_items(&ws.pool, &source_ids, filter, Utc::now()).await?,
    ))
}

async fn context_items(
    State(ws): State<AppState>,
    Path(context): Path<String>,
) -> ApiResult<Json<Vec<Item>>> {
    items(&ws, &context, Filter::Active).await
}

async fn context_snoozed(
    State(ws): State<AppState>,
    Path(context): Path<String>,
) -> ApiResult<Json<Vec<Item>>> {
    items(&ws, &context, Filter::Snoozed).await
}

async fn refresh_context(
    State(ws): State<AppState>,
    Path(context): Path<String>,
) -> ApiResult<StatusCode> {
    find_context(&ws, &context)?;
    futures::future::join_all(ws.context_sources(&context).map(|s| s.handle.refresh())).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn refresh_source(
    State(ws): State<AppState>,
    Path(source): Path<String>,
) -> ApiResult<StatusCode> {
    find_source(&ws, &source)?.handle.refresh().await;
    Ok(StatusCode::NO_CONTENT)
}

fn state_changed(ws: &Workspace, source_id: String, item_id: String) -> StatusCode {
    let _ = ws
        .events
        .send(Event::ItemStateChanged { source_id, item_id });
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
struct SnoozeBody {
    /// Snooze until this time, or forever if null.
    until: Option<DateTime<Utc>>,
}

async fn snooze_item(
    State(ws): State<AppState>,
    Path((source, item)): Path<(String, String)>,
    Json(body): Json<SnoozeBody>,
) -> ApiResult<StatusCode> {
    find_item(&ws, &source, &item).await?;
    db::set_snooze(&ws.pool, &source, &item, body.until).await?;
    Ok(state_changed(&ws, source, item))
}

async fn unsnooze_item(
    State(ws): State<AppState>,
    Path((source, item)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    find_item(&ws, &source, &item).await?;
    db::clear_snooze(&ws.pool, &source, &item).await?;
    Ok(state_changed(&ws, source, item))
}

#[derive(Deserialize)]
struct DueBody {
    due: DateTime<Utc>,
}

async fn set_due(
    State(ws): State<AppState>,
    Path((source, item)): Path<(String, String)>,
    Json(body): Json<DueBody>,
) -> ApiResult<StatusCode> {
    find_item(&ws, &source, &item).await?;
    db::set_due_override(&ws.pool, &source, &item, Some(body.due)).await?;
    Ok(state_changed(&ws, source, item))
}

async fn clear_due(
    State(ws): State<AppState>,
    Path((source, item)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    find_item(&ws, &source, &item).await?;
    db::set_due_override(&ws.pool, &source, &item, None).await?;
    Ok(state_changed(&ws, source, item))
}

async fn activity(State(ws): State<AppState>) -> StatusCode {
    if ws.activity.ping() {
        // The dashboard was idle; refresh everything.
        for source in &ws.sources {
            source.handle.wake();
        }
    }
    StatusCode::NO_CONTENT
}

async fn events(
    State(ws): State<AppState>,
) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    let stream = BroadcastStream::new(ws.events.subscribe()).filter_map(|event| async move {
        let event = event.ok()?;
        SseEvent::default().json_data(event).ok().map(Ok)
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}
