//! Tributary: unified task dashboard server.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::Utc;
use sqlx::SqlitePool;
use tokio::sync::watch;
use tracing::warn;

use crate::accounts::Accounts;
use crate::activity::Activity;
use crate::config::Config;
use crate::events::Events;
use crate::google::GoogleClient;
use crate::scheduler::{Schedule, SourceHandle};
use crate::settings::SourceRecord;
use crate::sources::{Registry, Source, SourceContext};

pub mod accounts;
pub mod activity;
pub mod api;
pub mod config;
pub mod db;
pub mod due;
pub mod events;
pub mod google;
pub mod scheduler;
pub mod settings;
pub mod sources;

/// A source that is being polled.
#[derive(Clone)]
pub struct RunningSource {
    pub icon: String,
    pub source: Arc<dyn Source>,
    pub handle: SourceHandle,
}

/// Everything the API needs: configuration, storage, the available source
/// types and the running sources.
///
/// There is currently a single workspace. When user accounts are added each
/// user would get their own.
pub struct Workspace {
    pub config: Config,
    pub pool: SqlitePool,
    pub events: Events,
    pub activity: Arc<Activity>,
    pub registry: Registry,
    pub accounts: Arc<Accounts>,
    running: Mutex<HashMap<String, RunningSource>>,
    /// Set to true when the server is shutting down.
    shutdown: watch::Sender<bool>,
}

impl Workspace {
    /// Creates the default context if needed and starts polling every stored
    /// source. Must be called within a tokio runtime. Polling stops when the
    /// workspace is dropped.
    pub async fn start(
        config: Config,
        pool: SqlitePool,
        registry: Registry,
    ) -> anyhow::Result<Arc<Self>> {
        settings::ensure_default_context(&pool).await?;
        let activity = Arc::new(Activity::new(config.polling.active_timeout));
        let google = config
            .google
            .clone()
            .map(|google| GoogleClient::new(google, &config.server.public_url));
        let events = events::channel();
        let accounts = Arc::new(Accounts::new(pool.clone(), google, events.clone()));
        let workspace = Arc::new(Self {
            config,
            pool,
            events,
            activity,
            registry,
            accounts,
            running: Mutex::new(HashMap::new()),
            shutdown: watch::Sender::new(false),
        });
        for source in settings::list_sources(&workspace.pool).await? {
            workspace.run(&source).await;
        }
        Ok(workspace)
    }

    /// Ends long-lived responses (event streams) so that a graceful server
    /// shutdown doesn't wait for them.
    pub fn shutdown(&self) {
        self.shutdown.send_replace(true);
    }

    /// Resolves once [`Workspace::shutdown`] has been called.
    async fn shutting_down(&self) {
        let _ = self
            .shutdown
            .subscribe()
            .wait_for(|shutdown| *shutdown)
            .await;
    }

    /// What sources get to build and fetch with. `wants_due_from` is
    /// whether the source has a default due date.
    pub fn source_context(&self, wants_due_from: bool) -> SourceContext {
        SourceContext {
            accounts: self.accounts.clone(),
            wants_due_from,
        }
    }

    /// Starts polling a source, replacing it if it was already running. If
    /// the source can't be built the failure is recorded in its status.
    pub async fn run(&self, record: &SourceRecord) {
        let built = match self.registry.get(&record.kind) {
            Some(source_type) => source_type.build(
                &record.config,
                &self.source_context(record.default_due.is_some()),
            ),
            None => Err(anyhow::anyhow!("unknown source type `{}`", record.kind)),
        };
        match built {
            Ok(source) => {
                let running = RunningSource {
                    icon: source.icon(),
                    source: source.clone(),
                    handle: scheduler::spawn(
                        Schedule {
                            source_id: record.id.clone(),
                            interval: self.config.polling.active,
                            idle_interval: self.config.polling.idle,
                            default_due: record.default_due,
                        },
                        source,
                        self.pool.clone(),
                        self.events.clone(),
                        self.activity.clone(),
                    ),
                };
                self.running
                    .lock()
                    .unwrap()
                    .insert(record.id.clone(), running);
            }
            Err(error) => {
                warn!(source = record.id, %error, "can't start source");
                self.stop(&record.id);
                let message = format!("{error:#}");
                if let Err(error) =
                    db::record_failure(&self.pool, &record.id, &message, Utc::now()).await
                {
                    warn!(source = record.id, %error, "failed to record status");
                }
            }
        }
    }

    /// Stops polling a source.
    pub fn stop(&self, id: &str) {
        self.running.lock().unwrap().remove(id);
    }

    pub fn running(&self, id: &str) -> Option<RunningSource> {
        self.running.lock().unwrap().get(id).cloned()
    }

    pub fn all_running(&self) -> Vec<RunningSource> {
        self.running.lock().unwrap().values().cloned().collect()
    }
}
