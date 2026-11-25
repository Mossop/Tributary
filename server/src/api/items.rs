//! Items, refreshing and change events.

use std::convert::Infallible;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use chrono::{DateTime, Utc};
use futures::{Stream, StreamExt};
use serde::Deserialize;
use tokio_stream::wrappers::BroadcastStream;

use super::{ApiError, ApiResult, AppState, find_context, find_source};
use crate::Workspace;
use crate::db::{self, Filter, Item};
use crate::events::Event;
use crate::settings;

async fn find_item(ws: &Workspace, source_id: &str, item_id: &str) -> ApiResult<()> {
    find_source(ws, source_id).await?;
    if db::item_exists(&ws.pool, source_id, item_id).await? {
        Ok(())
    } else {
        Err(ApiError::NotFound(format!("unknown item `{item_id}`")))
    }
}

async fn items(ws: &Workspace, context: &str, filter: Filter) -> ApiResult<Json<Vec<Item>>> {
    find_context(ws, context).await?;
    let source_ids = settings::context_source_ids(&ws.pool, context).await?;
    let source_ids: Vec<&str> = source_ids.iter().map(String::as_str).collect();
    Ok(Json(
        db::list_items(&ws.pool, &source_ids, filter, Utc::now()).await?,
    ))
}

pub async fn context_items(
    State(ws): State<AppState>,
    Path(context): Path<String>,
) -> ApiResult<Json<Vec<Item>>> {
    items(&ws, &context, Filter::Active).await
}

pub async fn context_snoozed(
    State(ws): State<AppState>,
    Path(context): Path<String>,
) -> ApiResult<Json<Vec<Item>>> {
    items(&ws, &context, Filter::Snoozed).await
}

pub async fn refresh_context(
    State(ws): State<AppState>,
    Path(context): Path<String>,
) -> ApiResult<StatusCode> {
    find_context(&ws, &context).await?;
    let handles: Vec<_> = settings::context_source_ids(&ws.pool, &context)
        .await?
        .iter()
        .filter_map(|id| ws.running(id))
        .map(|running| running.handle)
        .collect();
    futures::future::join_all(handles.iter().map(|h| h.refresh())).await;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn refresh_source(
    State(ws): State<AppState>,
    Path(source): Path<String>,
) -> ApiResult<StatusCode> {
    find_source(&ws, &source).await?;
    if let Some(running) = ws.running(&source) {
        running.handle.refresh().await;
    }
    Ok(StatusCode::NO_CONTENT)
}

fn state_changed(ws: &Workspace, source_id: String, item_id: String) -> StatusCode {
    let _ = ws
        .events
        .send(Event::ItemStateChanged { source_id, item_id });
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
pub struct SnoozeBody {
    /// Snooze until this time, or forever if null.
    until: Option<DateTime<Utc>>,
}

pub async fn snooze_item(
    State(ws): State<AppState>,
    Path((source, item)): Path<(String, String)>,
    Json(body): Json<SnoozeBody>,
) -> ApiResult<StatusCode> {
    find_item(&ws, &source, &item).await?;
    db::set_snooze(&ws.pool, &source, &item, body.until).await?;
    Ok(state_changed(&ws, source, item))
}

pub async fn unsnooze_item(
    State(ws): State<AppState>,
    Path((source, item)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    find_item(&ws, &source, &item).await?;
    db::clear_snooze(&ws.pool, &source, &item).await?;
    Ok(state_changed(&ws, source, item))
}

#[derive(Deserialize)]
pub struct DueBody {
    due: DateTime<Utc>,
}

pub async fn set_due(
    State(ws): State<AppState>,
    Path((source, item)): Path<(String, String)>,
    Json(body): Json<DueBody>,
) -> ApiResult<StatusCode> {
    find_item(&ws, &source, &item).await?;
    db::set_due_override(&ws.pool, &source, &item, Some(body.due)).await?;
    Ok(state_changed(&ws, source, item))
}

pub async fn clear_due(
    State(ws): State<AppState>,
    Path((source, item)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    find_item(&ws, &source, &item).await?;
    db::set_due_override(&ws.pool, &source, &item, None).await?;
    Ok(state_changed(&ws, source, item))
}

pub async fn activity(State(ws): State<AppState>) -> StatusCode {
    if ws.activity.ping() {
        // The dashboard was idle; refresh everything.
        for running in ws.all_running() {
            running.handle.wake();
        }
    }
    StatusCode::NO_CONTENT
}

pub async fn events(
    State(ws): State<AppState>,
) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    let stream = BroadcastStream::new(ws.events.subscribe()).filter_map(|event| async move {
        let event = event.ok()?;
        SseEvent::default().json_data(event).ok().map(Ok)
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}
