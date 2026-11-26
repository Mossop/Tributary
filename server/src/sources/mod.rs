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
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::accounts::Accounts;

pub mod bugzilla;
pub mod github;
pub mod google_tasks;
pub mod phabricator;
pub mod references;
pub mod stub;

/// What kind of thing an item is. Every item is one of these, so the
/// dashboard can show or hide each kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemKind {
    /// Something to do, e.g. a Google task.
    Task,
    /// A reported problem or request, e.g. a GitHub issue.
    Issue,
    /// A proposed change, e.g. a GitHub pull request.
    Patch,
}

impl ItemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ItemKind::Task => "task",
            ItemKind::Issue => "issue",
            ItemKind::Patch => "patch",
        }
    }

    pub fn parse(kind: &str) -> Option<Self> {
        match kind {
            "task" => Some(ItemKind::Task),
            "issue" => Some(ItemKind::Issue),
            "patch" => Some(ItemKind::Patch),
            _ => None,
        }
    }
}

/// An item produced by a source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceItem {
    /// Identifier, unique within the source and stable across fetches.
    pub id: String,
    pub kind: ItemKind,
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
    /// Quick actions offered for this item, performed by [`Source::perform`].
    #[serde(default)]
    pub actions: Vec<QuickAction>,
    /// The time a default due date counts from (e.g. when a review was
    /// requested), for sources whose settings give one. Ignored when `due`
    /// is set.
    #[serde(default)]
    pub due_from: Option<DateTime<Utc>>,
    /// Identifiers of what the item is about, including itself, e.g.
    /// `bugzilla:123`, `phab:D456` or `github:owner/repo:7` (see
    /// [`references`]). Items sharing any are grouped in the dashboard.
    #[serde(default)]
    pub references: Vec<String>,
}

/// Something that can be done to an item directly from the dashboard.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuickAction {
    /// Identifies the action to [`Source::perform`].
    pub id: String,
    /// Describes the action, e.g. "Mark complete".
    pub label: String,
    /// Either a URL or `symbol:<name>` for a Material Symbol.
    pub icon: String,
    /// Makes the action a link: the dashboard opens this URL in a new tab
    /// instead of asking the source to perform the action.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
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

    /// Performs one of an item's quick actions. The source is refreshed
    /// afterwards.
    async fn perform(&self, _item_id: &str, _action: &str) -> Result<(), SourceError> {
        Err(SourceError::Message(
            "this source has no quick actions".to_string(),
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldKind {
    Text,
    Url,
    /// Write-only: never returned by the API once set.
    Secret,
    /// The id of a connected account with the field's `provider`.
    Account,
    /// One of the options returned by the source type's options function.
    Select,
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
    /// For account fields, the account provider (e.g. `google`).
    pub provider: Option<&'static str>,
}

/// A choice for a select field.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FieldOption {
    pub value: String,
    pub label: String,
}

/// What sources can use while building and fetching.
#[derive(Clone)]
pub struct SourceContext {
    pub accounts: Arc<Accounts>,
    /// The source has a default due date, so items should set `due_from`.
    /// Sources can skip work needed only for that otherwise.
    pub wants_due_from: bool,
}

type BuildFn = dyn Fn(&Value, &SourceContext) -> anyhow::Result<Arc<dyn Source>> + Send + Sync;

/// Lists the options of a select field (`field`) given the rest of a
/// possibly incomplete config.
pub type OptionsFn = dyn Fn(&'static str, Value, SourceContext) -> BoxFuture<'static, anyhow::Result<Vec<FieldOption>>>
    + Send
    + Sync;

/// A kind of source that can be configured.
#[derive(Clone)]
pub struct SourceType {
    pub id: String,
    pub name: String,
    /// Whether the type is offered in the settings UI.
    pub listed: bool,
    pub fields: Vec<Field>,
    /// OAuth scopes needed from the accounts of each provider.
    pub scopes: Vec<(&'static str, &'static str)>,
    /// What items' `due_from` is, e.g. "after the request was made", for
    /// types offering a default due date.
    pub default_due: Option<&'static str>,
    build: Arc<BuildFn>,
    options: Option<Arc<OptionsFn>>,
}

impl SourceType {
    pub fn new(
        id: &str,
        name: &str,
        fields: Vec<Field>,
        build: impl Fn(&Value, &SourceContext) -> anyhow::Result<Arc<dyn Source>>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            listed: true,
            fields,
            scopes: Vec::new(),
            default_due: None,
            build: Arc::new(build),
            options: None,
        }
    }

    pub fn unlisted(mut self) -> Self {
        self.listed = false;
        self
    }

    /// Offers a default due date, counted from items' `due_from`, which is
    /// `description` (e.g. "after the request was made").
    pub fn with_default_due(mut self, description: &'static str) -> Self {
        self.default_due = Some(description);
        self
    }

    /// Declares an OAuth scope needed from `provider` accounts.
    pub fn with_scope(mut self, provider: &'static str, scope: &'static str) -> Self {
        self.scopes.push((provider, scope));
        self
    }

    /// Sets the function listing the options of select fields.
    pub fn with_options(
        mut self,
        options: impl Fn(
            &'static str,
            Value,
            SourceContext,
        ) -> BoxFuture<'static, anyhow::Result<Vec<FieldOption>>>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.options = Some(Arc::new(options));
        self
    }

    /// The OAuth scopes needed from `provider` accounts.
    pub fn scopes_for<'a>(&'a self, provider: &'a str) -> impl Iterator<Item = &'static str> + 'a {
        self.scopes
            .iter()
            .filter(move |(p, _)| *p == provider)
            .map(|(_, scope)| *scope)
    }

    /// Lists the options of the select field `key`. None if there is no
    /// such field.
    pub async fn options(
        &self,
        key: &str,
        config: Value,
        context: SourceContext,
    ) -> Option<anyhow::Result<Vec<FieldOption>>> {
        let field = self
            .fields
            .iter()
            .find(|f| f.key == key && f.kind == FieldKind::Select)?;
        let options = self.options.as_ref()?;
        Some(options(field.key, config, context).await)
    }

    /// Checks required fields and builds the source from its config.
    pub fn build(
        &self,
        config: &Value,
        context: &SourceContext,
    ) -> anyhow::Result<Arc<dyn Source>> {
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
        (self.build)(config, context)
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
        registry.register(google_tasks::source_type());
        registry.register(phabricator::source_type());
        registry.register(bugzilla::source_type());
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

    /// Every OAuth scope that listed source types need from `provider`
    /// accounts.
    pub fn scopes_for(&self, provider: &str) -> Vec<&'static str> {
        let mut scopes: Vec<&'static str> =
            self.listed().flat_map(|t| t.scopes_for(provider)).collect();
        scopes.sort_unstable();
        scopes.dedup();
        scopes
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
                    provider: None,
                },
                Field {
                    key: "token",
                    label: "Token",
                    kind: FieldKind::Secret,
                    required: true,
                    placeholder: None,
                    help: None,
                    provider: None,
                },
            ],
            |_, _| Ok(Arc::new(stub::StubSource::new(Vec::new()))),
        )
    }

    fn context() -> SourceContext {
        let pool = sqlx::SqlitePool::connect_lazy("sqlite::memory:").unwrap();
        SourceContext {
            accounts: Arc::new(Accounts::new(pool, None, crate::events::channel())),
            wants_due_from: false,
        }
    }

    #[tokio::test]
    async fn build_checks_required_fields() {
        let t = test_type();
        let cx = context();
        let err = t
            .build(&json!({ "query": " ", "token": "x" }), &cx)
            .err()
            .unwrap();
        assert_eq!(err.to_string(), "Query is required");
        let err = t.build(&json!({ "query": "q" }), &cx).err().unwrap();
        assert_eq!(err.to_string(), "Token is required");
        assert!(t.build(&json!({ "query": "q", "token": "x" }), &cx).is_ok());
    }

    #[tokio::test]
    async fn lists_select_options() {
        let mut t = test_type();
        t.fields.push(Field {
            key: "choice",
            label: "Choice",
            kind: FieldKind::Select,
            required: true,
            placeholder: None,
            help: None,
            provider: None,
        });
        let t = t.with_options(|key, config, _| {
            Box::pin(async move {
                Ok(vec![FieldOption {
                    value: key.to_string(),
                    label: config["query"].as_str().unwrap_or_default().to_string(),
                }])
            })
        });
        let options = t
            .options("choice", json!({ "query": "q" }), context())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            options,
            [FieldOption {
                value: "choice".into(),
                label: "q".into()
            }]
        );
        assert!(t.options("query", json!({}), context()).await.is_none());
        assert!(t.options("missing", json!({}), context()).await.is_none());
    }

    #[test]
    fn collects_scopes_of_listed_types() {
        let mut registry = Registry::default();
        registry.register(
            test_type()
                .with_scope("google", "b")
                .with_scope("other", "x"),
        );
        registry.register(
            SourceType::new("u", "U", vec![], |_, _| bail!("unused"))
                .with_scope("google", "a")
                .with_scope("google", "b"),
        );
        registry.register(
            SourceType::new("hidden", "H", vec![], |_, _| bail!("unused"))
                .with_scope("google", "c")
                .unlisted(),
        );
        assert_eq!(registry.scopes_for("google"), ["a", "b"]);
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
