//! Connecting and removing accounts.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Redirect;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::warn;

use super::{ApiError, ApiResult, AppState};
use crate::Workspace;
use crate::accounts::{self, GOOGLE};
use crate::events::Event;
use crate::google::IDENTITY_SCOPES;
use crate::settings::{self, SourceRecord};
use crate::sources::FieldKind;

#[derive(Serialize)]
pub struct ProviderInfo {
    id: &'static str,
    name: &'static str,
    /// A URL, or `symbol:<name>` for a Material Symbol.
    icon: &'static str,
    /// Whether the server has the configuration needed to connect accounts.
    configured: bool,
}

pub async fn list_providers(State(ws): State<AppState>) -> Json<Vec<ProviderInfo>> {
    Json(vec![ProviderInfo {
        id: GOOGLE,
        name: "Google",
        icon: "https://www.gstatic.com/images/branding/googleg/1x/googleg_standard_color_48dp.png",
        configured: ws.accounts.google().is_some(),
    }])
}

#[derive(Serialize)]
pub struct AccountInfo {
    #[serde(flatten)]
    account: accounts::Account,
    /// The account lacks scopes that source types need, so it should be
    /// connected again.
    missing_scopes: bool,
    /// Access was revoked or expired, so it must be connected again.
    revoked: bool,
}

pub async fn list_accounts(State(ws): State<AppState>) -> ApiResult<Json<Vec<AccountInfo>>> {
    let accounts = accounts::list_accounts(&ws.pool).await?;
    Ok(Json(
        accounts
            .into_iter()
            .map(|account| AccountInfo {
                missing_scopes: !account.has_scopes(ws.registry.scopes_for(&account.provider)),
                revoked: ws.accounts.is_revoked(&account.id),
                account,
            })
            .collect(),
    ))
}

/// The sources configured to use an account.
async fn sources_using(ws: &Workspace, account_id: &str) -> ApiResult<Vec<SourceRecord>> {
    let sources = settings::list_sources(&ws.pool).await?;
    Ok(sources
        .into_iter()
        .filter(|source| {
            ws.registry.get(&source.kind).is_some_and(|t| {
                t.fields.iter().any(|f| {
                    f.kind == FieldKind::Account
                        && source.config.get(f.key).and_then(Value::as_str) == Some(account_id)
                })
            })
        })
        .collect())
}

pub async fn delete_account(
    State(ws): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    if accounts::get_account(&ws.pool, &id).await?.is_none() {
        return Err(ApiError::NotFound(format!("unknown account `{id}`")));
    }
    let users = sources_using(&ws, &id).await?;
    if !users.is_empty() {
        let names: Vec<&str> = users.iter().map(|s| s.name.as_str()).collect();
        return Err(ApiError::Conflict(format!(
            "these sources use this account, so change or delete them first: {}",
            names.join(", ")
        )));
    }
    accounts::delete_account(&ws.pool, &id).await?;
    ws.accounts.forget(&id);
    let _ = ws.events.send(Event::SettingsChanged);
    Ok(StatusCode::NO_CONTENT)
}

/// Back to the settings page, optionally reporting an error.
fn to_settings(error: Option<&str>) -> Redirect {
    match error {
        None => Redirect::to("/settings"),
        Some(error) => {
            let mut url = Url::parse("http://localhost/settings").unwrap();
            url.query_pairs_mut().append_pair("error", error);
            Redirect::to(&format!(
                "{}?{}",
                url.path(),
                url.query().unwrap_or_default()
            ))
        }
    }
}

#[derive(Deserialize)]
pub struct ConnectQuery {
    /// Reconnect this account.
    account: Option<String>,
}

/// Sends the browser to Google to grant access.
pub async fn connect_google(
    State(ws): State<AppState>,
    Query(query): Query<ConnectQuery>,
) -> ApiResult<Redirect> {
    let Some(google) = ws.accounts.google() else {
        return Ok(to_settings(Some(
            "Google isn't configured on the server; see the [google] settings",
        )));
    };
    let login_hint = match &query.account {
        Some(id) => accounts::get_account(&ws.pool, id).await?.map(|a| a.name),
        None => None,
    };
    let mut scopes: Vec<String> = IDENTITY_SCOPES.iter().map(|s| s.to_string()).collect();
    scopes.extend(ws.registry.scopes_for(GOOGLE).into_iter().map(String::from));
    let state = ws.accounts.begin_connect();
    let url = google.authorize_url(&state, &scopes, login_hint.as_deref());
    Ok(Redirect::to(url.as_str()))
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

/// Where Google sends the browser back to after the user grants access.
pub async fn google_callback(
    State(ws): State<AppState>,
    Query(query): Query<CallbackQuery>,
) -> Redirect {
    match finish_google_connect(&ws, query).await {
        Ok(()) => to_settings(None),
        Err(error) => {
            warn!(%error, "couldn't connect Google account");
            to_settings(Some(&error))
        }
    }
}

async fn finish_google_connect(ws: &Workspace, query: CallbackQuery) -> Result<(), String> {
    let valid_state = query
        .state
        .as_deref()
        .is_some_and(|state| ws.accounts.finish_connect(state));
    if !valid_state {
        return Err("The Google sign in expired, please try again".to_string());
    }
    if let Some(error) = query.error {
        return Err(if error == "access_denied" {
            "Access to the Google account wasn't granted".to_string()
        } else {
            format!("Google returned an error: {error}")
        });
    }
    let code = query.code.ok_or("Google didn't return a code")?;
    let google = ws
        .accounts
        .google()
        .ok_or("Google isn't configured on the server")?;

    let failed =
        |error: crate::google::GoogleError| format!("Couldn't connect the Google account: {error}");
    let tokens = google.exchange_code(&code).await.map_err(failed)?;
    let refresh_token = tokens
        .refresh_token
        .as_deref()
        .ok_or("Google didn't grant offline access, please try again")?;
    let user = google
        .user_info(&tokens.access_token)
        .await
        .map_err(failed)?;

    let id = accounts::save_account(
        &ws.pool,
        GOOGLE,
        &user.sub,
        &user.email,
        refresh_token,
        tokens.scope.as_deref().unwrap_or_default(),
    )
    .await
    .map_err(|error| format!("Couldn't save the account: {error}"))?;
    ws.accounts.forget(&id);

    // Sources may have been failing because the account needed
    // reconnecting.
    if let Ok(sources) = sources_using(ws, &id).await {
        for source in sources {
            if let Some(running) = ws.running(&source.id) {
                running.handle.wake();
            }
        }
    }
    let _ = ws.events.send(Event::SettingsChanged);
    Ok(())
}
