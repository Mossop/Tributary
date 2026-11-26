//! Lists Bugzilla bugs: open bugs assigned to me, or bugs with a flag
//! requested of me.

use std::sync::Arc;

use anyhow::{anyhow, bail};
use async_trait::async_trait;
use futures::{StreamExt as _, TryStreamExt as _, stream};
use serde_json::Value;

use super::{
    Field, FieldKind, FieldOption, ITEM_REQUESTS, ItemKind, Source, SourceContext, SourceError,
    SourceItem, SourceType, references,
};
use crate::accounts::{Accounts, BUGZILLA};

pub mod client;

use client::{Bug, Bugzilla, BugzillaError, Flag};

pub const ICON: &str = "https://bugzilla.mozilla.org/favicon.ico";

/// Always offered, as it's the most common request.
const NEEDINFO: &str = "needinfo";
const ANY_FLAG: &str = "*";

/// What a source shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Show {
    /// Open bugs assigned to me.
    Assigned,
    /// Bugs with this flag requested of me, or any flag for `None`.
    Requests(Option<String>),
}

impl Show {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "assigned" => Some(Show::Assigned),
            _ => match value.strip_prefix("request:")? {
                "" => None,
                ANY_FLAG => Some(Show::Requests(None)),
                flag => Some(Show::Requests(Some(flag.to_string()))),
            },
        }
    }
}

fn request_option(flag: &str) -> FieldOption {
    FieldOption {
        value: format!("request:{flag}"),
        label: format!("Requests: {flag}"),
    }
}

pub fn source_type() -> SourceType {
    SourceType::new(
        "bugzilla",
        "Bugzilla",
        vec![
            Field {
                key: "account",
                label: "Bugzilla account",
                kind: FieldKind::Account,
                required: true,
                placeholder: None,
                help: None,
                provider: Some(BUGZILLA),
            },
            Field {
                key: "show",
                label: "Show",
                kind: FieldKind::Select,
                required: true,
                placeholder: None,
                help: Some("Requests list bugs with that flag requested of you."),
                provider: None,
            },
        ],
        |config, context| {
            let account = config["account"].as_str().unwrap_or_default().to_string();
            let show = config["show"]
                .as_str()
                .and_then(Show::parse)
                .ok_or_else(|| anyhow!("Choose what to show"))?;
            Ok(Arc::new(BugzillaSource::new(context, account, show)?))
        },
    )
    .with_requested("Request made (requests)")
    .with_options(|_, config, context| {
        Box::pin(async move {
            let mut flags = vec![NEEDINFO.to_string()];
            // Offer the flags recently requested of me, if an account is
            // chosen.
            if let Some(account) = config.get("account").and_then(Value::as_str) {
                let (client, _) = client(&context.accounts, account).await?;
                let me = client.whoami().await?.name;
                for flag in client.requested_flag_names(&me).await? {
                    if !flags.contains(&flag) {
                        flags.push(flag);
                    }
                }
            }
            let mut options = vec![FieldOption {
                value: "assigned".to_string(),
                label: "Bugs assigned to me".to_string(),
            }];
            options.extend(flags.iter().map(|f| request_option(f)));
            options.push(FieldOption {
                value: format!("request:{ANY_FLAG}"),
                label: "Requests: any flag".to_string(),
            });
            Ok(options)
        })
    })
}

/// A client for an account, and the account's name.
async fn client(accounts: &Accounts, account: &str) -> anyhow::Result<(Bugzilla, String)> {
    let (account, key) = accounts.token(account, BUGZILLA, "Bugzilla").await?;
    let base_url = client::base_url(account.api_url.as_deref());
    Ok((Bugzilla::new(http()?, &base_url, &key), account.name))
}

pub fn http() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder().user_agent("tributary").build()
}

pub struct BugzillaSource {
    accounts: Arc<Accounts>,
    account: String,
    show: Show,
}

impl BugzillaSource {
    pub fn new(context: &SourceContext, account: String, show: Show) -> anyhow::Result<Self> {
        Ok(Self {
            accounts: context.accounts.clone(),
            account,
            show,
        })
    }

    async fn items(&self) -> anyhow::Result<Vec<SourceItem>> {
        let (client, name) = client(&self.accounts, &self.account).await?;
        let result = async {
            let me = client.whoami().await?.name;
            match &self.show {
                Show::Assigned => assigned(&client, &me).await,
                Show::Requests(flag) => requests(&client, &me, flag.as_deref()).await,
            }
        }
        .await;
        match result {
            Err(BugzillaError::Rejected) => {
                self.accounts.mark_revoked(&self.account);
                bail!("Bugzilla account {name} needs reconnecting in settings")
            }
            result => Ok(result?),
        }
    }
}

fn item(client: &Bugzilla, bug: &Bug, description: String) -> SourceItem {
    SourceItem {
        id: bug.id.to_string(),
        kind: ItemKind::Issue,
        title: bug.summary.clone(),
        url: client.bug_url(bug.id),
        icon: None,
        modified: bug.last_change_time,
        created: bug.creation_time,
        description: Some(description),
        due: None,
        actions: Vec::new(),
        due_from: None,
        references: vec![references::bugzilla(bug.id)],
    }
}

/// Open bugs assigned to me.
async fn assigned(client: &Bugzilla, me: &str) -> Result<Vec<SourceItem>, BugzillaError> {
    let bugs = client
        .search(&[
            ("f1", "assigned_to".to_string()),
            ("o1", "equals".to_string()),
            ("v1", me.to_string()),
            ("status", "__open__".to_string()),
            (
                "include_fields",
                "id,summary,status,product,component,creation_time,last_change_time".to_string(),
            ),
        ])
        .await?;
    Ok(bugs
        .iter()
        .map(|bug| {
            let description = format!(
                "Bug {} · {} :: {} · {}",
                bug.id, bug.product, bug.component, bug.status
            );
            item(client, bug, description)
        })
        .collect())
}

/// Whether `flag` is a request of me for `wanted` (any flag for `None`).
fn requested_of_me(flag: &Flag, me: &str, wanted: Option<&str>) -> bool {
    flag.status == "?"
        && flag.requestee.as_deref() == Some(me)
        && wanted.is_none_or(|name| flag.name == name)
}

/// Bugs with `flag` (or any flag) requested of me.
async fn requests(
    client: &Bugzilla,
    me: &str,
    flag: Option<&str>,
) -> Result<Vec<SourceItem>, BugzillaError> {
    let mut query = vec![
        ("f1", "requestees.login_name".to_string()),
        ("o1", "equals".to_string()),
        ("v1", me.to_string()),
        (
            "include_fields",
            "id,summary,status,product,component,creation_time,last_change_time,flags".to_string(),
        ),
    ];
    if let Some(flag) = flag {
        query.push(("f2", "flagtypes.name".to_string()));
        query.push(("o2", "equals".to_string()));
        query.push(("v2", format!("{flag}?")));
    }
    let bugs = client.search(&query).await?;

    // Owned bugs: borrowed ones trip a compiler limitation in the
    // `async_trait` future.
    let items: Vec<Option<SourceItem>> = stream::iter(bugs)
        .map(|bug| async move { request_item(client, me, flag, &bug).await })
        .buffered(ITEM_REQUESTS)
        .try_collect()
        .await?;
    Ok(items.into_iter().flatten().collect())
}

/// The item for a bug with requests of me, if it has any.
async fn request_item(
    client: &Bugzilla,
    me: &str,
    flag: Option<&str>,
    bug: &Bug,
) -> Result<Option<SourceItem>, BugzillaError> {
    // The search matches attachment flags too, which aren't in the bug's
    // flags.
    let mut matching: Vec<(&Flag, bool)> = bug
        .flags
        .iter()
        .filter(|f| requested_of_me(f, me, flag))
        .map(|f| (f, false))
        .collect();
    let attachment_flags;
    if matching.is_empty() {
        attachment_flags = client.attachment_flags(bug.id).await?;
        matching = attachment_flags
            .iter()
            .filter(|f| requested_of_me(f, me, flag))
            .map(|f| (f, true))
            .collect();
    }
    if matching.is_empty() {
        return Ok(None);
    }
    let requests: Vec<String> = matching
        .iter()
        .map(|(f, on_attachment)| {
            let mut request = format!("{}?", f.name);
            if let Some(setter) = &f.setter {
                request.push_str(&format!(" from {setter}"));
            }
            if *on_attachment {
                request.push_str(" (attachment)");
            }
            request
        })
        .collect();
    let mut item = item(
        client,
        bug,
        format!("Bug {} · {}", bug.id, requests.join(", ")),
    );
    // When I was asked.
    if let Some(asked) = matching.iter().map(|(f, _)| f.modification_date).max() {
        item.modified = asked;
    }
    // A default due date counts from the oldest request.
    item.due_from = matching.iter().map(|(f, _)| f.creation_date).min();
    Ok(Some(item))
}

#[async_trait]
impl Source for BugzillaSource {
    fn icon(&self) -> String {
        ICON.to_string()
    }

    async fn fetch(&self) -> Result<Vec<SourceItem>, SourceError> {
        self.items()
            .await
            .map_err(|error| SourceError::Message(format!("{error:#}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_what_to_show() {
        assert_eq!(Show::parse("assigned"), Some(Show::Assigned));
        assert_eq!(
            Show::parse("request:needinfo"),
            Some(Show::Requests(Some("needinfo".into())))
        );
        assert_eq!(Show::parse("request:*"), Some(Show::Requests(None)));
        assert_eq!(Show::parse("request:"), None);
        assert_eq!(Show::parse("bogus"), None);
    }

    #[test]
    fn matches_requests_of_me() {
        let flag = |name: &str, status: &str, requestee: Option<&str>| Flag {
            name: name.into(),
            status: status.into(),
            setter: None,
            requestee: requestee.map(Into::into),
            creation_date: chrono::DateTime::UNIX_EPOCH,
            modification_date: chrono::DateTime::UNIX_EPOCH,
        };
        let me = "me@example.com";
        assert!(requested_of_me(
            &flag("needinfo", "?", Some(me)),
            me,
            Some("needinfo")
        ));
        assert!(requested_of_me(&flag("review", "?", Some(me)), me, None));
        assert!(!requested_of_me(
            &flag("review", "?", Some(me)),
            me,
            Some("needinfo")
        ));
        assert!(!requested_of_me(
            &flag("needinfo", "?", Some("other@example.com")),
            me,
            None
        ));
        assert!(!requested_of_me(&flag("needinfo", "+", Some(me)), me, None));
        assert!(!requested_of_me(&flag("needinfo", "?", None), me, None));
    }
}
