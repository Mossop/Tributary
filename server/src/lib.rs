//! Tributary: unified task dashboard server.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::Utc;
use sqlx::SqlitePool;
use tracing::warn;

use crate::activity::Activity;
use crate::config::Config;
use crate::events::Events;
use crate::scheduler::{Schedule, SourceHandle};
use crate::settings::SourceRecord;
use crate::sources::Registry;

pub mod activity;
pub mod api;
pub mod config;
pub mod db;
pub mod events;
pub mod scheduler;
pub mod settings;
pub mod sources;

/// A source that is being polled.
#[derive(Clone)]
pub struct RunningSource {
    pub icon: String,
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
    running: Mutex<HashMap<String, RunningSource>>,
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
        let workspace = Arc::new(Self {
            config,
            pool,
            events: events::channel(),
            activity,
            registry,
            running: Mutex::new(HashMap::new()),
        });
        for source in settings::list_sources(&workspace.pool).await? {
            workspace.run(&source).await;
        }
        Ok(workspace)
    }

    /// Starts polling a source, replacing it if it was already running. If
    /// the source can't be built the failure is recorded in its status.
    pub async fn run(&self, record: &SourceRecord) {
        let built = match self.registry.get(&record.kind) {
            Some(source_type) => source_type.build(&record.config),
            None => Err(anyhow::anyhow!("unknown source type `{}`", record.kind)),
        };
        match built {
            Ok(source) => {
                let running = RunningSource {
                    icon: source.icon(),
                    handle: scheduler::spawn(
                        Schedule {
                            source_id: record.id.clone(),
                            interval: self.config.polling.active,
                            idle_interval: self.config.polling.idle,
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
