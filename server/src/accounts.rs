//! Accounts with external services that sources use, and access tokens for
//! them.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context as _, anyhow, bail};
use serde::Serialize;
use sqlx::SqlitePool;
use tokio::time::Instant;

use crate::events::{Event, Events};
use crate::google::{GoogleClient, GoogleError};

pub const GOOGLE: &str = "google";

/// How long a connect attempt can take before its state expires.
const STATE_LIFETIME: Duration = Duration::from_secs(10 * 60);
/// Access tokens are refreshed this long before they expire.
const EXPIRY_MARGIN: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Serialize, sqlx::FromRow)]
pub struct Account {
    pub id: String,
    pub provider: String,
    #[serde(skip)]
    pub subject: String,
    pub name: String,
    /// Granted OAuth scopes, space separated.
    #[serde(skip)]
    pub scopes: String,
}

impl Account {
    pub fn has_scopes<'a>(&self, scopes: impl IntoIterator<Item = &'a str>) -> bool {
        let granted: Vec<&str> = self.scopes.split_whitespace().collect();
        scopes.into_iter().all(|scope| granted.contains(&scope))
    }
}

pub async fn list_accounts(pool: &SqlitePool) -> sqlx::Result<Vec<Account>> {
    sqlx::query_as(
        "SELECT id, provider, subject, name, scopes FROM accounts ORDER BY provider, name",
    )
    .fetch_all(pool)
    .await
}

pub async fn get_account(pool: &SqlitePool, id: &str) -> sqlx::Result<Option<Account>> {
    sqlx::query_as("SELECT id, provider, subject, name, scopes FROM accounts WHERE id = ?1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

async fn refresh_token(pool: &SqlitePool, id: &str) -> sqlx::Result<Option<String>> {
    sqlx::query_scalar("SELECT refresh_token FROM accounts WHERE id = ?1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// Adds an account, or updates it if the provider's account was already
/// connected. Returns the account's id.
pub async fn save_account(
    pool: &SqlitePool,
    provider: &str,
    subject: &str,
    name: &str,
    refresh_token: &str,
    scopes: &str,
) -> sqlx::Result<String> {
    let id = uuid::Uuid::new_v4().simple().to_string();
    sqlx::query_scalar(
        "INSERT INTO accounts (id, provider, subject, name, refresh_token, scopes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (provider, subject) DO UPDATE SET
           name = excluded.name,
           refresh_token = excluded.refresh_token,
           scopes = excluded.scopes
         RETURNING id",
    )
    .bind(id)
    .bind(provider)
    .bind(subject)
    .bind(name)
    .bind(refresh_token)
    .bind(scopes)
    .fetch_one(pool)
    .await
}

pub async fn delete_account(pool: &SqlitePool, id: &str) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM accounts WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

struct AccessToken {
    token: String,
    expires: Instant,
}

/// Connected accounts: hands out access tokens to sources and tracks connect
/// attempts in progress.
pub struct Accounts {
    pool: SqlitePool,
    google: Option<GoogleClient>,
    tokens: Mutex<HashMap<String, AccessToken>>,
    /// Connect attempts in progress: OAuth state to when it expires.
    pending: Mutex<HashMap<String, Instant>>,
    /// Accounts whose access was revoked or expired, found when refreshing
    /// their access token. Not stored: after a restart sources fetch
    /// straight away and find out again.
    revoked: Mutex<HashSet<String>>,
    events: Events,
}

impl Accounts {
    pub fn new(pool: SqlitePool, google: Option<GoogleClient>, events: Events) -> Self {
        Self {
            pool,
            google,
            tokens: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
            revoked: Mutex::new(HashSet::new()),
            events,
        }
    }

    /// Whether the account's access was revoked or expired, so it must be
    /// connected again.
    pub fn is_revoked(&self, account_id: &str) -> bool {
        self.revoked.lock().unwrap().contains(account_id)
    }

    fn set_revoked(&self, account_id: &str) {
        if self.revoked.lock().unwrap().insert(account_id.to_string()) {
            // Clients show which accounts need reconnecting.
            let _ = self.events.send(Event::SettingsChanged);
        }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// The Google OAuth client, if Google is configured.
    pub fn google(&self) -> Option<&GoogleClient> {
        self.google.as_ref()
    }

    /// Starts a connect attempt, returning its OAuth state.
    pub fn begin_connect(&self) -> String {
        let state = uuid::Uuid::new_v4().simple().to_string();
        let now = Instant::now();
        let mut pending = self.pending.lock().unwrap();
        pending.retain(|_, expires| *expires > now);
        pending.insert(state.clone(), now + STATE_LIFETIME);
        state
    }

    /// Ends a connect attempt. False if the state is unknown or expired.
    pub fn finish_connect(&self, state: &str) -> bool {
        self.pending
            .lock()
            .unwrap()
            .remove(state)
            .is_some_and(|expires| expires > Instant::now())
    }

    /// Forgets the cached access token and revocation for an account, e.g.
    /// after it was reconnected or removed.
    pub fn forget(&self, account_id: &str) {
        self.tokens.lock().unwrap().remove(account_id);
        self.revoked.lock().unwrap().remove(account_id);
    }

    /// An access token for the account, refreshed if needed.
    pub async fn access_token(&self, account_id: &str) -> anyhow::Result<String> {
        if let Some(token) = self.tokens.lock().unwrap().get(account_id)
            && token.expires > Instant::now()
        {
            return Ok(token.token.clone());
        }

        let account = get_account(&self.pool, account_id)
            .await?
            .ok_or_else(|| anyhow!("the account was removed"))?;
        if account.provider != GOOGLE {
            bail!("unknown account provider `{}`", account.provider);
        }
        let google = self
            .google
            .as_ref()
            .ok_or_else(|| anyhow!("Google isn't configured on the server"))?;
        let refresh_token = refresh_token(&self.pool, account_id)
            .await?
            .context("the account was removed")?;
        let tokens = match google.refresh(&refresh_token).await {
            Ok(tokens) => tokens,
            Err(GoogleError::Revoked) => {
                self.set_revoked(account_id);
                bail!(
                    "Google account {} needs reconnecting in settings",
                    account.name
                );
            }
            Err(error) => {
                return Err(anyhow!(error).context("couldn't refresh the Google access token"));
            }
        };

        let expires = Instant::now() + tokens.expires_in().saturating_sub(EXPIRY_MARGIN);
        self.tokens.lock().unwrap().insert(
            account_id.to_string(),
            AccessToken {
                token: tokens.access_token.clone(),
                expires,
            },
        );
        Ok(tokens.access_token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[tokio::test]
    async fn saves_and_updates_accounts() {
        let pool = db::connect_in_memory().await.unwrap();
        let id = save_account(&pool, GOOGLE, "sub1", "a@example.com", "r1", "openid")
            .await
            .unwrap();
        let again = save_account(&pool, GOOGLE, "sub1", "b@example.com", "r2", "openid email")
            .await
            .unwrap();
        assert_eq!(id, again);
        let accounts = list_accounts(&pool).await.unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].name, "b@example.com");
        assert_eq!(
            refresh_token(&pool, &id).await.unwrap().as_deref(),
            Some("r2")
        );

        save_account(&pool, GOOGLE, "sub2", "c@example.com", "r3", "")
            .await
            .unwrap();
        assert_eq!(list_accounts(&pool).await.unwrap().len(), 2);

        delete_account(&pool, &id).await.unwrap();
        assert_eq!(get_account(&pool, &id).await.unwrap(), None);
    }

    #[test]
    fn checks_scopes() {
        let account = Account {
            id: "a".into(),
            provider: GOOGLE.into(),
            subject: "s".into(),
            name: "n".into(),
            scopes: "openid email https://www.googleapis.com/auth/tasks.readonly".into(),
        };
        assert!(account.has_scopes(["email", "https://www.googleapis.com/auth/tasks.readonly"]));
        assert!(!account.has_scopes(["https://www.googleapis.com/auth/gmail.readonly"]));
    }

    async fn accounts() -> Accounts {
        Accounts::new(
            db::connect_in_memory().await.unwrap(),
            None,
            crate::events::channel(),
        )
    }

    #[tokio::test]
    async fn connect_states_are_single_use() {
        let accounts = accounts().await;
        let state = accounts.begin_connect();
        assert!(!accounts.finish_connect("other"));
        assert!(accounts.finish_connect(&state));
        assert!(!accounts.finish_connect(&state));
    }

    #[tokio::test]
    async fn connect_states_expire() {
        let accounts = accounts().await;
        tokio::time::pause();
        let state = accounts.begin_connect();
        tokio::time::advance(STATE_LIFETIME + Duration::from_secs(1)).await;
        assert!(!accounts.finish_connect(&state));
    }
}
