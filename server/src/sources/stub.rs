//! A source whose items are defined directly in configuration. Useful for
//! trying out the dashboard without credentials and for tests.

use async_trait::async_trait;
use serde::Deserialize;

use super::{Source, SourceError, SourceItem};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct StubConfig {
    #[serde(default)]
    pub items: Vec<SourceItem>,
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
