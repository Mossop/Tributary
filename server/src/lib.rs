//! Unified task dashboard server.

use std::sync::Arc;

use sqlx::SqlitePool;

use crate::activity::Activity;
use crate::config::{Config, SourceCommon};
use crate::events::Events;
use crate::scheduler::SourceHandle;
use crate::sources::Source;

pub mod activity;
pub mod api;
pub mod config;
pub mod db;
pub mod events;
pub mod scheduler;
pub mod sources;

/// A source that is being polled.
pub struct SourceEntry {
    pub common: SourceCommon,
    pub icon: String,
    pub handle: SourceHandle,
}

/// Everything the API needs: configuration, storage and the running sources.
///
/// There is currently a single workspace. When user accounts are added each
/// user would get their own.
pub struct Workspace {
    pub config: Config,
    pub pool: SqlitePool,
    pub events: Events,
    pub activity: Arc<Activity>,
    pub sources: Vec<SourceEntry>,
}

impl Workspace {
    /// Starts polling the given sources. Must be called within a tokio
    /// runtime. Polling stops when the workspace is dropped.
    pub fn start(
        config: Config,
        sources: Vec<(SourceCommon, Arc<dyn Source>)>,
        pool: SqlitePool,
    ) -> Arc<Self> {
        let events = events::channel();
        let activity = Arc::new(Activity::new(config.server.active_timeout));
        let sources = sources
            .into_iter()
            .map(|(common, source)| {
                let icon = source.icon();
                let handle = scheduler::spawn(
                    common.clone(),
                    source,
                    pool.clone(),
                    events.clone(),
                    activity.clone(),
                );
                SourceEntry {
                    common,
                    icon,
                    handle,
                }
            })
            .collect();

        Arc::new(Self {
            config,
            pool,
            events,
            activity,
            sources,
        })
    }

    pub fn source(&self, id: &str) -> Option<&SourceEntry> {
        self.sources.iter().find(|s| s.common.id == id)
    }

    /// The sources displayed in a context.
    pub fn context_sources(&self, context_id: &str) -> impl Iterator<Item = &SourceEntry> {
        self.sources
            .iter()
            .filter(move |s| s.common.contexts.iter().any(|c| c == context_id))
    }
}

/// Builds the source implementations for every configured source.
pub fn build_sources(config: &Config) -> anyhow::Result<Vec<(SourceCommon, Arc<dyn Source>)>> {
    config
        .sources
        .iter()
        .map(|source| Ok((source.common.clone(), sources::build_source(source)?)))
        .collect()
}
