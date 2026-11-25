//! Change notifications pushed to connected clients.

use serde::Serialize;
use tokio::sync::broadcast;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Event {
    /// A source finished a fetch; its items and/or status may have changed.
    SourceUpdated { source_id: String },
    /// The user's state (snooze, due override) for an item changed.
    ItemStateChanged { source_id: String, item_id: String },
    /// Contexts or sources were added, changed or removed.
    SettingsChanged,
}

pub type Events = broadcast::Sender<Event>;

pub fn channel() -> Events {
    broadcast::channel(256).0
}
