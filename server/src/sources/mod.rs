//! The source API contract.
//!
//! A source is anything that can produce a list of items for the dashboard.
//! Each configured source is polled periodically by the scheduler which calls
//! [`Source::fetch`] and stores the result, replacing whatever items the source
//! previously returned.
//!
//! # Adding a new source
//!
//! 1. Create a module in `sources/` with a config struct (deserialized from the
//!    source's `[[sources]]` table) and a type implementing [`Source`].
//! 2. Add a variant to [`crate::config::SourceKind`]; the variant name in
//!    lowercase is the `type` used in configuration.
//! 3. Add a match arm to [`build_source`].

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::config::{SourceConfig, SourceKind};

pub mod github;
pub mod stub;

/// An item produced by a source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceItem {
    /// Identifier, unique within the source and stable across fetches.
    pub id: String,
    pub title: String,
    /// Where to view the full details of the item.
    pub url: String,
    /// A custom icon for this item. When absent the source's icon is used.
    #[serde(default)]
    pub icon: Option<String>,
    pub modified: DateTime<Utc>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub due: Option<DateTime<Utc>>,
}

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
}

#[async_trait]
pub trait Source: Send + Sync {
    /// The icon representing this source and its items. Either a URL or
    /// `symbol:<name>` for a Material Symbol.
    fn icon(&self) -> String;

    /// Fetches the current complete list of items.
    async fn fetch(&self) -> Result<Vec<SourceItem>, SourceError>;
}

/// Constructs the source implementation for a configured source.
pub fn build_source(config: &SourceConfig) -> anyhow::Result<Arc<dyn Source>> {
    Ok(match &config.kind {
        SourceKind::Github(github) => Arc::new(github::GithubSource::from_config(github)?),
        SourceKind::Stub(stub) => Arc::new(stub::StubSource::new(stub.items.clone())),
    })
}
