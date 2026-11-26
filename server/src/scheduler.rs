//! Periodic polling of sources.
//!
//! Each running source gets its own task which fetches immediately on start,
//! then waits for the source's `interval` (while the dashboard is active) or
//! `idle_interval` (while idle) before fetching again. A fetch can be forced
//! at any time through the source's [`SourceHandle`]. Dropping every handle
//! stops the task.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use sqlx::SqlitePool;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, warn};

use crate::activity::Activity;
use crate::db;
use crate::due::DefaultDue;
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

pub struct Schedule {
    pub source_id: String,
    pub interval: Duration,
    pub idle_interval: Duration,
    /// Given to items without a due date.
    pub default_due: Option<DefaultDue>,
}

/// Spawns the polling task for a source.
pub fn spawn(
    schedule: Schedule,
    source: Arc<dyn Source>,
    pool: SqlitePool,
    events: Events,
    activity: Arc<Activity>,
) -> SourceHandle {
    let (tx, mut rx) = mpsc::unbounded_channel::<Request>();

    tokio::spawn(async move {
        let id = schedule.source_id.as_str();
        let mut waiters = Vec::new();
        loop {
            // Coalesce any requests that arrived while waiting.
            while let Ok(request) = rx.try_recv() {
                waiters.extend(request);
            }

            let mut result = fetch(id, source.as_ref()).await;
            if let (Ok(items), Some(default_due)) = (&mut result, schedule.default_due) {
                apply_default_due(items, default_due);
            }
            // The source was removed or replaced while fetching; don't store
            // stale results.
            if rx.is_closed() {
                break;
            }
            store(id, result, &pool, &events).await;
            for waiter in waiters.drain(..) {
                let _ = waiter.send(());
            }

            let wait = if activity.is_active() {
                schedule.interval
            } else {
                schedule.idle_interval
            };
            tokio::select! {
                _ = tokio::time::sleep(wait) => {}
                request = rx.recv() => match request {
                    Some(request) => waiters.extend(request),
                    None => break,
                },
            }
        }
        debug!(source = id, "stopped");
    });

    SourceHandle { tx }
}

/// Gives items without a due date one counted from the time `default_due`
/// says.
fn apply_default_due(items: &mut [crate::sources::SourceItem], default_due: DefaultDue) {
    for item in items {
        if item.due.is_none() {
            item.due = default_due.due(item);
        }
    }
}

async fn fetch(id: &str, source: &dyn Source) -> Result<Vec<crate::sources::SourceItem>, String> {
    debug!(source = id, "fetching");
    source.fetch().await.map_err(|error| {
        warn!(source = id, %error, "fetch failed");
        error.to_string()
    })
}

async fn store(
    id: &str,
    result: Result<Vec<crate::sources::SourceItem>, String>,
    pool: &SqlitePool,
    events: &Events,
) {
    let stored = match result {
        Ok(items) => {
            debug!(source = id, count = items.len(), "fetched");
            db::replace_items(pool, id, &items, Utc::now()).await
        }
        Err(error) => db::record_failure(pool, id, &error, Utc::now()).await,
    };
    if let Err(error) = stored {
        warn!(source = id, %error, "failed to store fetch result");
    }
    let _ = events.send(Event::SourceUpdated {
        source_id: id.to_string(),
    });
}
