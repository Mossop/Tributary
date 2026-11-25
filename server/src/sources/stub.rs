//! A source with fixed items, given in its config as an `items` array. Not
//! offered in the settings UI; useful for tests and development.

use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;

use super::{Source, SourceError, SourceItem, SourceType};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct StubConfig {
    #[serde(default)]
    pub items: Vec<SourceItem>,
}

pub fn source_type() -> SourceType {
    SourceType::new("stub", "Stub", Vec::new(), |config, _| {
        let config: StubConfig = serde_json::from_value(config.clone())?;
        Ok(Arc::new(StubSource::new(config.items)))
    })
    .unlisted()
}

pub struct StubSource {
    items: Vec<SourceItem>,
}

impl StubSource {
    pub fn new(items: Vec<SourceItem>) -> Self {
        Self { items }
    }
}

#[async_trait]
impl Source for StubSource {
    fn icon(&self) -> String {
        "symbol:checklist".to_string()
    }

    async fn fetch(&self) -> Result<Vec<SourceItem>, SourceError> {
        Ok(self.items.clone())
    }
}
