//! Managing contexts and sources.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{ApiError, ApiResult, AppState, find_context, find_source};
use crate::Workspace;
use crate::accounts;
use crate::db::{self, SourceStatus};
use crate::due::{self, DefaultDue};
use crate::events::Event;
use crate::settings::{self, Context, SourceRecord};
use crate::sources::{Field, FieldKind, FieldOption, ItemKind, SourceType};

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
        // Set separately, with `set_hidden_kinds`.
        hidden_kinds: Vec::new(),
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
    Ok((
        StatusCode::CREATED,
        Json(find_context(&ws, &context.id).await?),
    ))
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
    Ok(Json(find_context(&ws, &context.id).await?))
}

/// Sets which item kinds a context hides.
pub async fn set_hidden_kinds(
    State(ws): State<AppState>,
    Path(id): Path<String>,
    Json(mut kinds): Json<Vec<ItemKind>>,
) -> ApiResult<Json<Context>> {
    find_context(&ws, &id).await?;
    kinds.sort_by_key(|k| k.as_str());
    kinds.dedup();
    settings::set_hidden_kinds(&ws.pool, &id, &kinds).await?;
    settings_changed(&ws);
    Ok(Json(find_context(&ws, &id).await?))
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
    /// A URL, or `symbol:<name>` for a Material Symbol.
    #[serde(skip_serializing_if = "Option::is_none")]
    icon: Option<&'static str>,
    fields: Vec<Field>,
    /// The times a default due date can count from, the default first.
    due_from: Vec<FieldOption>,
}

pub async fn list_source_types(State(ws): State<AppState>) -> Json<Vec<SourceTypeInfo>> {
    Json(
        ws.registry
            .listed()
            .map(|t| SourceTypeInfo {
                id: t.id.clone(),
                name: t.name.clone(),
                icon: t.icon,
                fields: t.fields.clone(),
                due_from: t
                    .due_from_choices()
                    .into_iter()
                    .map(|(from, label)| FieldOption {
                        value: from.as_str().to_string(),
                        label: label.to_string(),
                    })
                    .collect(),
            })
            .collect(),
    )
}

#[derive(Deserialize)]
pub struct OptionsBody {
    #[serde(default)]
    config: Map<String, Value>,
}

/// The options of a source type's select field, given the rest of the
/// (possibly incomplete) config.
pub async fn field_options(
    State(ws): State<AppState>,
    Path((kind, field)): Path<(String, String)>,
    Json(body): Json<OptionsBody>,
) -> ApiResult<Json<Vec<FieldOption>>> {
    let source_type = ws
        .registry
        .get(&kind)
        .ok_or_else(|| ApiError::NotFound(format!("unknown source type `{kind}`")))?;
    let options = source_type
        .options(&field, Value::Object(body.config), ws.source_context(false))
        .await
        .ok_or_else(|| ApiError::NotFound(format!("unknown select field `{field}`")))?;
    options
        .map(Json)
        .map_err(|error| ApiError::BadRequest(format!("{error:#}")))
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
    default_due: Option<DefaultDue>,
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
        default_due: record.default_due,
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
    /// Absent or null for none.
    #[serde(default)]
    default_due: Option<DefaultDue>,
}

/// Highest number of days a default due date can be.
const MAX_DUE_DAYS: u32 = 365;

fn validate_default_due(source_type: &SourceType, body: &SourceBody) -> ApiResult<()> {
    let Some(default_due) = body.default_due else {
        return Ok(());
    };
    if !source_type
        .due_from_choices()
        .iter()
        .any(|(from, _)| *from == default_due.from)
    {
        return Err(ApiError::BadRequest(format!(
            "{} sources can't count default due dates from `{}`",
            source_type.name,
            default_due.from.as_str()
        )));
    }
    if !(1..=MAX_DUE_DAYS).contains(&default_due.days) {
        return Err(ApiError::BadRequest(format!(
            "The default due date must be between 1 and {MAX_DUE_DAYS} days"
        )));
    }
    Ok(())
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

/// Checks that account fields name a connected account of the right
/// provider.
async fn validate_accounts(
    ws: &Workspace,
    source_type: &SourceType,
    config: &Value,
) -> ApiResult<()> {
    for field in source_type
        .fields
        .iter()
        .filter(|f| f.kind == FieldKind::Account)
    {
        let Some(id) = config
            .get(field.key)
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
        else {
            continue;
        };
        let account = accounts::get_account(&ws.pool, id).await?;
        if account.is_none_or(|a| Some(a.provider.as_str()) != field.provider) {
            return Err(ApiError::BadRequest(format!("unknown account `{id}`")));
        }
    }
    Ok(())
}

async fn record_from(
    ws: &Workspace,
    id: String,
    source_type: &SourceType,
    body: SourceBody,
    config: Value,
) -> ApiResult<SourceRecord> {
    validate_accounts(ws, source_type, &config).await?;
    validate_default_due(source_type, &body)?;
    source_type
        .build(
            &config,
            &ws.source_context(due::counts_from_requested(body.default_due)),
        )
        .map_err(|error| ApiError::BadRequest(format!("{error:#}")))?;
    Ok(SourceRecord {
        id,
        kind: source_type.id.clone(),
        name: body.name.trim().to_string(),
        contexts: body.contexts,
        config,
        default_due: body.default_due,
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
    let record = record_from(&ws, id, &source_type, body, config).await?;
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
    let record = record_from(&ws, id, &source_type, body, config).await?;
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
