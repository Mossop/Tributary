//! The source API contract.
//!
//! A source is anything that can produce a list of items for the dashboard.
//! Each configured source is polled periodically by the scheduler which calls
//! [`Source::fetch`] and stores the result, replacing whatever items the source
//! previously returned.
//!
//! Sources are configured at runtime through the settings UI. Settings common
//! to all sources (name, contexts) are handled by the
//! dashboard. Settings specific to a source type are stored as a JSON object
//! described by the type's [`Field`]s, from which the settings UI builds its
//! form.
//!
//! # Adding a new source type
//!
//! 1. Create a module in `sources/` with a `Deserialize` config struct and a
//!    type implementing [`Source`].
//! 2. Register a [`SourceType`] in [`Registry::builtin`] describing its
//!    fields and how to build it from its JSON config.

use std::sync::Arc;

use anyhow::bail;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldKind {
    Text,
    Url,
    /// Write-only: never returned by the API once set.
    Secret,
}

/// A setting specific to a source type.
#[derive(Debug, Clone, Serialize)]
pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: FieldKind,
    pub required: bool,
    pub placeholder: Option<&'static str>,
    pub help: Option<&'static str>,
}

type BuildFn = dyn Fn(&Value) -> anyhow::Result<Arc<dyn Source>> + Send + Sync;

/// A kind of source that can be configured.
#[derive(Clone)]
pub struct SourceType {
    pub id: String,
    pub name: String,
    /// Whether the type is offered in the settings UI.
    pub listed: bool,
    pub fields: Vec<Field>,
    build: Arc<BuildFn>,
}

impl SourceType {
    pub fn new(
        id: &str,
        name: &str,
        fields: Vec<Field>,
        build: impl Fn(&Value) -> anyhow::Result<Arc<dyn Source>> + Send + Sync + 'static,
    ) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            listed: true,
            fields,
            build: Arc::new(build),
        }
    }

    pub fn unlisted(mut self) -> Self {
        self.listed = false;
        self
    }

    /// Checks required fields and builds the source from its config.
    pub fn build(&self, config: &Value) -> anyhow::Result<Arc<dyn Source>> {
        for field in self.fields.iter().filter(|f| f.required) {
            let present = match config.get(field.key) {
                Some(Value::String(s)) => !s.trim().is_empty(),
                Some(Value::Null) | None => false,
                Some(_) => true,
            };
            if !present {
                bail!("{} is required", field.label);
            }
        }
        (self.build)(config)
    }

    fn is_secret(&self, key: &str) -> bool {
        self.fields
            .iter()
            .any(|f| f.key == key && f.kind == FieldKind::Secret)
    }

    /// Splits a stored config into the parts safe to return to clients and
    /// the keys of secrets that are set.
    pub fn public_config(&self, config: &Value) -> (Map<String, Value>, Vec<String>) {
        let mut public = Map::new();
        let mut secrets = Vec::new();
        if let Value::Object(map) = config {
            for (key, value) in map {
                if self.is_secret(key) {
                    if !value.is_null() && value != "" {
                        secrets.push(key.clone());
                    }
                } else {
                    public.insert(key.clone(), value.clone());
                }
            }
        }
        (public, secrets)
    }

    /// Applies an update from a client to a stored config. Secrets that the
    /// client leaves empty or omits keep their stored value.
    pub fn merge_config(&self, stored: &Value, update: Map<String, Value>) -> Value {
        let mut merged = update;
        if let Value::Object(stored) = stored {
            for (key, value) in stored {
                let unset = matches!(merged.get(key), None | Some(Value::Null))
                    || merged.get(key).is_some_and(|v| v == "");
                if self.is_secret(key) && unset {
                    merged.insert(key.clone(), value.clone());
                }
            }
        }
        Value::Object(merged)
    }
}

/// The source types available to this dashboard.
#[derive(Clone, Default)]
pub struct Registry {
    types: Vec<SourceType>,
}

impl Registry {
    pub fn builtin() -> Self {
        let mut registry = Self::default();
        registry.register(github::source_type());
        registry.register(stub::source_type());
        registry
    }

    pub fn register(&mut self, source_type: SourceType) {
        self.types.retain(|t| t.id != source_type.id);
        self.types.push(source_type);
    }

    pub fn get(&self, id: &str) -> Option<&SourceType> {
        self.types.iter().find(|t| t.id == id)
    }

    pub fn listed(&self) -> impl Iterator<Item = &SourceType> {
        self.types.iter().filter(|t| t.listed)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn test_type() -> SourceType {
        SourceType::new(
            "t",
            "Test",
            vec![
                Field {
                    key: "query",
                    label: "Query",
                    kind: FieldKind::Text,
                    required: true,
                    placeholder: None,
                    help: None,
                },
                Field {
                    key: "token",
                    label: "Token",
                    kind: FieldKind::Secret,
                    required: true,
                    placeholder: None,
                    help: None,
                },
            ],
            |_| Ok(Arc::new(stub::StubSource::new(Vec::new()))),
        )
    }

    #[test]
    fn build_checks_required_fields() {
        let t = test_type();
        let err = t
            .build(&json!({ "query": " ", "token": "x" }))
            .err()
            .unwrap();
        assert_eq!(err.to_string(), "Query is required");
        let err = t.build(&json!({ "query": "q" })).err().unwrap();
        assert_eq!(err.to_string(), "Token is required");
        assert!(t.build(&json!({ "query": "q", "token": "x" })).is_ok());
    }

    #[test]
    fn public_config_hides_secrets() {
        let t = test_type();
        let (public, secrets) = t.public_config(&json!({ "query": "q", "token": "x" }));
        assert_eq!(Value::Object(public), json!({ "query": "q" }));
        assert_eq!(secrets, ["token"]);

        let (_, secrets) = t.public_config(&json!({ "query": "q", "token": "" }));
        assert!(secrets.is_empty());
    }

    #[test]
    fn merge_keeps_unset_secrets() {
        let t = test_type();
        let stored = json!({ "query": "old", "token": "secret" });

        let update = json!({ "query": "new" }).as_object().unwrap().clone();
        assert_eq!(
            t.merge_config(&stored, update),
            json!({ "query": "new", "token": "secret" })
        );

        let update = json!({ "query": "new", "token": "" })
            .as_object()
            .unwrap()
            .clone();
        assert_eq!(
            t.merge_config(&stored, update),
            json!({ "query": "new", "token": "secret" })
        );

        let update = json!({ "query": "new", "token": "changed" })
            .as_object()
            .unwrap()
            .clone();
        assert_eq!(
            t.merge_config(&stored, update),
            json!({ "query": "new", "token": "changed" })
        );
    }
}
