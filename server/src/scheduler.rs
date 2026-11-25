//! Periodic polling of sources.
//!
//! Each source gets its own task which fetches immediately on start, then
//! waits for the source's `interval` (while the dashboard is active) or
//! `idle_interval` (while idle) before fetching again. A fetch can be forced
//! at any time through the source's [`SourceHandle`].

use std::sync::Arc;

use chrono::Utc;
use sqlx::SqlitePool;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, warn};

use crate::activity::Activity;
use crate::config::SourceCommon;
use crate::db;
use crate::events::{Event, Events};
use crate::sources::Source;

type Request = Option<oneshot::Sender<()>>;

/// Allows forcing a refresh of a running source.
#[derive(Clone)]
pub struct SourceHandle {
    tx: mpsc::UnboundedSender<Request>,
}

impl SourceHandle {
    /// Requests a fetch without waiting for it.
    pub fn wake(&self) {
        let _ = self.tx.send(None);
    }

    /// Requests a fetch and waits for it to complete.
    pub async fn refresh(&self) {
        let (tx, rx) = oneshot::channel();
        if self.tx.send(Some(tx)).is_ok() {
            let _ = rx.await;
        }
    }
}

/// Spawns the polling task for a source.
pub fn spawn(
    common: SourceCommon,
    source: Arc<dyn Source>,
    pool: SqlitePool,
    events: Events,
    activity: Arc<Activity>,
) -> SourceHandle {
    let (tx, mut rx) = mpsc::unbounded_channel::<Request>();

    tokio::spawn(async move {
        let mut waiters = Vec::new();
        loop {
            // Coalesce any requests that arrived while waiting.
            while let Ok(request) = rx.try_recv() {
                waiters.extend(request);
            }

            fetch(&common, source.as_ref(), &pool, &events).await;
            for waiter in waiters.drain(..) {
                let _ = waiter.send(());
            }

            let wait = if activity.is_active() {
                common.interval
            } else {
                common.idle_interval
            };
            tokio::select! {
                _ = tokio::time::sleep(wait) => {}
                request = rx.recv() => match request {
                    Some(request) => waiters.extend(request),
                    // All handles dropped, the app has shut down.
                    None => break,
                },
            }
        }
    });

    SourceHandle { tx }
}

async fn fetch(common: &SourceCommon, source: &dyn Source, pool: &SqlitePool, events: &Events) {
    debug!(source = common.id, "fetching");
    let result = match source.fetch().await {
        Ok(items) => {
            debug!(source = common.id, count = items.len(), "fetched");
            db::replace_items(pool, &common.id, &items, Utc::now()).await
        }
        Err(error) => {
            warn!(source = common.id, %error, "fetch failed");
            db::record_failure(pool, &common.id, &error.to_string(), Utc::now()).await
        }
    };
    if let Err(error) = result {
        warn!(source = common.id, %error, "failed to store fetch result");
    }
    let _ = events.send(Event::SourceUpdated {
        source_id: common.id.clone(),
    });
}
