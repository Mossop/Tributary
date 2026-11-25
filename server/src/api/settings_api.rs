//! Managing contexts and sources.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{ApiError, ApiResult, AppState, find_context, find_source};
use crate::Workspace;
use crate::db::{self, SourceStatus};
use crate::events::Event;
use crate::settings::{self, Context, SourceRecord};
use crate::sources::{Field, SourceType};

fn settings_changed(ws: &Workspace) {
    let _ = ws.events.send(Event::SettingsChanged);
}

pub async fn list_contexts(State(ws): State<AppState>) -> ApiResult<Json<Vec<Context>>> {
    Ok(Json(settings::list_contexts(&ws.pool).await?))
}

fn valid_slug(slug: &str) -> bool {
    let mut chars = slug.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn validate_context(body: Context) -> ApiResult<Context> {
    let context = Context {
        id: body.id.trim().to_string(),
        name: body.name.trim().to_string(),
        icon: body
            .icon
            .map(|i| i.trim().to_string())
            .filter(|i| !i.is_empty()),
    };
    if !valid_slug(&context.id) {
        return Err(ApiError::BadRequest(
            "URL name must be lowercase letters, digits and dashes".to_string(),
        ));
    }
    if context.name.is_empty() {
        return Err(ApiError::BadRequest("Name is required".to_string()));
    }
    Ok(context)
}

async fn ensure_context_id_free(ws: &Workspace, id: &str) -> ApiResult<()> {
    if settings::get_context(&ws.pool, id).await?.is_some() {
        return Err(ApiError::Conflict(format!(
            "a context with URL name `{id}` already exists"
        )));
    }
    Ok(())
}

pub async fn create_context(
    State(ws): State<AppState>,
    Json(body): Json<Context>,
) -> ApiResult<(StatusCode, Json<Context>)> {
    let context = validate_context(body)?;
    ensure_context_id_free(&ws, &context.id).await?;
    settings::insert_context(&ws.pool, &context).await?;
    settings_changed(&ws);
    Ok((StatusCode::CREATED, Json(context)))
}

pub async fn update_context(
    State(ws): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Context>,
) -> ApiResult<Json<Context>> {
    find_context(&ws, &id).await?;
    let context = validate_context(body)?;
    if context.id != id {
        ensure_context_id_free(&ws, &context.id).await?;
    }
    settings::update_context(&ws.pool, &id, &context).await?;
    settings_changed(&ws);
    Ok(Json(context))
}

pub async fn delete_context(
    State(ws): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    find_context(&ws, &id).await?;
    if settings::list_contexts(&ws.pool).await?.len() == 1 {
        return Err(ApiError::Conflict(
            "the last context can't be deleted".to_string(),
        ));
    }
    let stranded = settings::sources_only_in_context(&ws.pool, &id).await?;
    if !stranded.is_empty() {
        return Err(ApiError::Conflict(format!(
            "these sources are only shown in this context, so move or delete them first: {}",
            stranded.join(", ")
        )));
    }
    settings::delete_context(&ws.pool, &id).await?;
    settings_changed(&ws);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
pub struct SourceTypeInfo {
    id: String,
    name: String,
    fields: Vec<Field>,
}

pub async fn list_source_types(State(ws): State<AppState>) -> Json<Vec<SourceTypeInfo>> {
    Json(
        ws.registry
            .listed()
            .map(|t| SourceTypeInfo {
                id: t.id.clone(),
                name: t.name.clone(),
                fields: t.fields.clone(),
            })
            .collect(),
    )
}

#[derive(Serialize)]
pub struct SourceInfo {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    name: String,
    /// Empty if the source isn't running (e.g. its config is invalid).
    icon: String,
    contexts: Vec<String>,
    /// Type specific settings, without secrets.
    config: Map<String, Value>,
    /// Keys of the secret settings that have a value.
    secrets_set: Vec<String>,
    status: SourceStatus,
}

fn source_info(ws: &Workspace, record: SourceRecord, status: SourceStatus) -> SourceInfo {
    let (config, secrets_set) = match ws.registry.get(&record.kind) {
        Some(source_type) => source_type.public_config(&record.config),
        None => (Map::new(), Vec::new()),
    };
    SourceInfo {
        icon: ws.running(&record.id).map(|r| r.icon).unwrap_or_default(),
        id: record.id,
        kind: record.kind,
        name: record.name,
        contexts: record.contexts,
        config,
        secrets_set,
        status,
    }
}

async fn load_source_info(ws: &Workspace, id: &str) -> ApiResult<SourceInfo> {
    let record = find_source(ws, id).await?;
    let status = db::source_statuses(&ws.pool)
        .await?
        .remove(id)
        .unwrap_or_default();
    Ok(source_info(ws, record, status))
}

pub async fn list_sources(State(ws): State<AppState>) -> ApiResult<Json<Vec<SourceInfo>>> {
    let mut statuses = db::source_statuses(&ws.pool).await?;
    let sources = settings::list_sources(&ws.pool).await?;
    Ok(Json(
        sources
            .into_iter()
            .map(|record| {
                let status = statuses.remove(&record.id).unwrap_or_default();
                source_info(&ws, record, status)
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
pub struct SourceBody {
    /// Required when creating; can't be changed afterwards.
    #[serde(rename = "type", default)]
    kind: Option<String>,
    name: String,
    contexts: Vec<String>,
    #[serde(default)]
    config: Map<String, Value>,
}

/// Validates the settings common to all sources.
async fn validate_common(ws: &Workspace, body: &SourceBody) -> ApiResult<()> {
    if body.name.trim().is_empty() {
        return Err(ApiError::BadRequest("Name is required".to_string()));
    }
    if body.contexts.is_empty() {
        return Err(ApiError::BadRequest(
            "Choose at least one context".to_string(),
        ));
    }
    for context in &body.contexts {
        find_context(ws, context)
            .await
            .map_err(|_| ApiError::BadRequest(format!("unknown context `{context}`")))?;
    }
    Ok(())
}

fn record_from(
    id: String,
    source_type: &SourceType,
    body: SourceBody,
    config: Value,
) -> ApiResult<SourceRecord> {
    source_type
        .build(&config)
        .map_err(|error| ApiError::BadRequest(format!("{error:#}")))?;
    Ok(SourceRecord {
        id,
        kind: source_type.id.clone(),
        name: body.name.trim().to_string(),
        contexts: body.contexts,
        config,
    })
}

async fn save_and_run(ws: &Workspace, record: SourceRecord) -> ApiResult<SourceInfo> {
    settings::save_source(&ws.pool, &record).await?;
    ws.run(&record).await;
    settings_changed(ws);
    load_source_info(ws, &record.id).await
}

pub async fn create_source(
    State(ws): State<AppState>,
    Json(body): Json<SourceBody>,
) -> ApiResult<(StatusCode, Json<SourceInfo>)> {
    validate_common(&ws, &body).await?;
    let kind = body
        .kind
        .clone()
        .ok_or_else(|| ApiError::BadRequest("type is required".to_string()))?;
    let source_type = ws
        .registry
        .get(&kind)
        .ok_or_else(|| ApiError::BadRequest(format!("unknown source type `{kind}`")))?
        .clone();
    let config = Value::Object(body.config.clone());
    let id = uuid::Uuid::new_v4().simple().to_string();
    let record = record_from(id, &source_type, body, config)?;
    Ok((StatusCode::CREATED, Json(save_and_run(&ws, record).await?)))
}

pub async fn update_source(
    State(ws): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SourceBody>,
) -> ApiResult<Json<SourceInfo>> {
    let stored = find_source(&ws, &id).await?;
    validate_common(&ws, &body).await?;
    if body.kind.as_ref().is_some_and(|kind| *kind != stored.kind) {
        return Err(ApiError::BadRequest(
            "a source's type can't be changed".to_string(),
        ));
    }
    let source_type = ws
        .registry
        .get(&stored.kind)
        .ok_or_else(|| ApiError::BadRequest(format!("unknown source type `{}`", stored.kind)))?
        .clone();
    let config = source_type.merge_config(&stored.config, body.config.clone());
    let record = record_from(id, &source_type, body, config)?;
    Ok(Json(save_and_run(&ws, record).await?))
}

pub async fn delete_source(
    State(ws): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    find_source(&ws, &id).await?;
    ws.stop(&id);
    settings::delete_source(&ws.pool, &id).await?;
    settings_changed(&ws);
    Ok(StatusCode::NO_CONTENT)
}
