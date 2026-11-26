//! Lists Gmail messages matching a search, with quick actions to mark them
//! read and to archive them.

use std::sync::Arc;

use anyhow::{Context as _, anyhow};
use async_trait::async_trait;
use chrono::DateTime;
use futures::{StreamExt as _, TryStreamExt as _, stream};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    Field, FieldKind, ITEM_REQUESTS, ItemKind, QuickAction, Source, SourceContext, SourceError,
    SourceItem, SourceType, references,
};
use crate::accounts::{self, Accounts, GOOGLE};

const ICON: &str = "https://www.gstatic.com/images/branding/product/1x/gmail_2020q4_48dp.png";
/// Reading messages and changing their labels, which marking them read and
/// archiving them needs.
const SCOPE: &str = "https://www.googleapis.com/auth/gmail.modify";
const MARK_READ: &str = "mark-read";
const ARCHIVE: &str = "archive";
const UNREAD: &str = "UNREAD";
const INBOX: &str = "INBOX";
/// The most messages a source lists.
const MAX_MESSAGES: &str = "100";

#[derive(Debug, Clone, Deserialize)]
pub struct GmailConfig {
    pub account: String,
    pub query: String,
}

pub fn source_type() -> SourceType {
    SourceType::new(
        "gmail",
        "Gmail",
        vec![
            Field {
                key: "account",
                label: "Google account",
                kind: FieldKind::Account,
                required: true,
                placeholder: None,
                help: None,
                provider: Some(GOOGLE),
            },
            Field {
                key: "query",
                label: "Search",
                kind: FieldKind::Text,
                required: true,
                placeholder: Some("is:unread in:inbox"),
                help: Some("Any Gmail search. Shows up to 100 messages, newest first."),
                provider: None,
            },
        ],
        |config, context| {
            let config: GmailConfig = serde_json::from_value(config.clone())?;
            Ok(Arc::new(GmailSource::new(context, config)?))
        },
    )
    .with_icon(ICON)
    .with_scope(GOOGLE, SCOPE)
}

#[derive(Deserialize)]
struct MessageList {
    #[serde(default)]
    messages: Vec<MessageRef>,
}

#[derive(Deserialize)]
struct MessageRef {
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Message {
    id: String,
    thread_id: String,
    #[serde(default)]
    label_ids: Vec<String>,
    #[serde(default)]
    snippet: String,
    /// Milliseconds since the epoch, as a string.
    internal_date: String,
    #[serde(default)]
    payload: Payload,
}

#[derive(Default, Deserialize)]
struct Payload {
    #[serde(default)]
    headers: Vec<Header>,
}

#[derive(Deserialize)]
struct Header {
    name: String,
    value: String,
}

impl Message {
    fn header(&self, name: &str) -> Option<&str> {
        self.payload
            .headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value.as_str())
    }

    /// The item for a message in `email`'s mailbox.
    fn into_item(self, email: &str) -> SourceItem {
        let subject = self
            .header("Subject")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("(No subject)")
            .to_string();
        let snippet = decode_entities(&self.snippet);
        let description = match self.header("From").map(sender) {
            Some(from) if snippet.is_empty() => from,
            Some(from) => format!("{from} · {snippet}"),
            None => snippet.clone(),
        };
        let time = self
            .internal_date
            .parse()
            .ok()
            .and_then(DateTime::from_timestamp_millis)
            .unwrap_or_default();
        let mut actions = Vec::new();
        if self.label_ids.iter().any(|l| l == UNREAD) {
            actions.push(QuickAction {
                id: MARK_READ.to_string(),
                label: "Mark as read".to_string(),
                icon: "symbol:mark_email_read".to_string(),
                url: None,
            });
        }
        if self.label_ids.iter().any(|l| l == INBOX) {
            actions.push(QuickAction {
                id: ARCHIVE.to_string(),
                label: "Archive".to_string(),
                icon: "symbol:archive".to_string(),
                url: None,
            });
        }
        let refs = references::in_text(&format!("{subject}\n{snippet}"));
        SourceItem {
            url: format!(
                "https://mail.google.com/mail/?authuser={}#all/{}",
                query_value(email),
                self.thread_id
            ),
            id: self.id,
            kind: ItemKind::Task,
            title: subject,
            icon: None,
            modified: time,
            created: Some(time),
            description: (!description.is_empty()).then_some(description),
            due: None,
            actions,
            due_from: None,
            references: refs,
        }
    }
}

/// The sender's name from a `From` header (`"Name" <address>`), or the
/// address if there's no name.
fn sender(from: &str) -> String {
    match from.split_once('<') {
        Some((name, address)) => {
            let name = name.trim().trim_matches('"').trim();
            if name.is_empty() {
                address.trim_end_matches('>').trim().to_string()
            } else {
                name.to_string()
            }
        }
        None => from.trim().to_string(),
    }
}

/// Gmail snippets escape HTML special characters.
fn decode_entities(text: &str) -> String {
    text.replace("&#39;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// Percent-encodes a query string value.
fn query_value(value: &str) -> String {
    let mut url = reqwest::Url::parse("http://x/").unwrap();
    url.query_pairs_mut().append_pair("v", value);
    url.query().unwrap_or_default()[2..].to_string()
}

pub struct GmailSource {
    accounts: Arc<Accounts>,
    account: String,
    query: String,
    http: reqwest::Client,
    api_url: String,
}

impl GmailSource {
    pub fn new(context: &SourceContext, config: GmailConfig) -> anyhow::Result<Self> {
        let google = context
            .accounts
            .google()
            .ok_or_else(|| anyhow!("Google isn't configured on the server"))?;
        if config.query.trim().is_empty() {
            return Err(anyhow!("Search is required"));
        }
        Ok(Self {
            accounts: context.accounts.clone(),
            account: config.account,
            query: config.query,
            http: google.http().clone(),
            api_url: google
                .config
                .gmail_api_url
                .trim_end_matches('/')
                .to_string(),
        })
    }

    async fn get<T: serde::de::DeserializeOwned>(
        &self,
        token: &str,
        path: &str,
        query: &[(&str, &str)],
    ) -> anyhow::Result<T> {
        let response = self
            .http
            .get(format!("{}/users/me{path}", self.api_url))
            .bearer_auth(token)
            .query(query)
            .send()
            .await?;
        let response = check(response).await?;
        response
            .json()
            .await
            .context("unexpected response from Gmail")
    }

    async fn items(&self) -> anyhow::Result<Vec<SourceItem>> {
        let token = self.accounts.access_token(&self.account).await?;
        let account = accounts::get_account(self.accounts.pool(), &self.account)
            .await?
            .ok_or_else(|| anyhow!("unknown account `{}`", self.account))?;
        let list: MessageList = self
            .get(
                &token,
                "/messages",
                &[("q", self.query.as_str()), ("maxResults", MAX_MESSAGES)],
            )
            .await?;
        let token = &token;
        let messages: Vec<Message> = stream::iter(list.messages)
            .map(|message| async move {
                self.get(
                    token,
                    &format!("/messages/{}", message.id),
                    &[
                        ("format", "metadata"),
                        ("metadataHeaders", "Subject"),
                        ("metadataHeaders", "From"),
                    ],
                )
                .await
            })
            .buffered(ITEM_REQUESTS)
            .try_collect()
            .await?;
        Ok(messages
            .into_iter()
            .map(|m| m.into_item(&account.name))
            .collect())
    }

    async fn run_action(&self, item_id: &str, action: &str) -> anyhow::Result<()> {
        let label = match action {
            MARK_READ => UNREAD,
            ARCHIVE => INBOX,
            _ => return Err(anyhow!("unknown action `{action}`")),
        };
        let token = self.accounts.access_token(&self.account).await?;
        let response = self
            .http
            .post(format!(
                "{}/users/me/messages/{item_id}/modify",
                self.api_url
            ))
            .bearer_auth(&token)
            .json(&json!({ "removeLabelIds": [label] }))
            .send()
            .await?;
        check(response).await?;
        Ok(())
    }
}

/// Turns an unsuccessful response into an error with Gmail's message.
async fn check(response: reqwest::Response) -> anyhow::Result<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body: Value = response.json().await.unwrap_or_default();
    let message = body["error"]["message"].as_str().unwrap_or_default();
    Err(anyhow!("Gmail returned {status}: {message}"))
}

#[async_trait]
impl Source for GmailSource {
    fn icon(&self) -> String {
        ICON.to_string()
    }

    async fn fetch(&self) -> Result<Vec<SourceItem>, SourceError> {
        self.items()
            .await
            .map_err(|error| SourceError::Message(format!("{error:#}")))
    }

    async fn perform(&self, item_id: &str, action: &str) -> Result<(), SourceError> {
        self.run_action(item_id, action)
            .await
            .map_err(|error| SourceError::Message(format!("{error:#}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(labels: &[&str]) -> Message {
        serde_json::from_value(json!({
            "id": "m1",
            "threadId": "t1",
            "labelIds": labels,
            "snippet": "Can you look at bug 1234? It&#39;s urgent &amp; important",
            "internalDate": "1795694773000",
            "payload": { "headers": [
                { "name": "Subject", "value": "Help with https://bugzilla.mozilla.org/show_bug.cgi?id=42" },
                { "name": "From", "value": "\"Alice Example\" <alice@example.com>" },
            ]},
        }))
        .unwrap()
    }

    #[test]
    fn maps_messages() {
        let item = message(&["INBOX", "UNREAD"]).into_item("me+work@example.com");
        assert_eq!(item.id, "m1");
        assert_eq!(item.kind, ItemKind::Task);
        assert_eq!(
            item.title,
            "Help with https://bugzilla.mozilla.org/show_bug.cgi?id=42"
        );
        assert_eq!(
            item.description.as_deref(),
            Some("Alice Example · Can you look at bug 1234? It's urgent & important")
        );
        assert_eq!(
            item.url,
            "https://mail.google.com/mail/?authuser=me%2Bwork%40example.com#all/t1"
        );
        assert_eq!(item.modified.timestamp(), 1_795_694_773);
        assert_eq!(item.created, Some(item.modified));
        assert_eq!(item.references, ["bugzilla:42"]);
    }

    #[test]
    fn offers_actions_by_label() {
        let ids = |labels: &[&str]| -> Vec<String> {
            message(labels)
                .into_item("me@example.com")
                .actions
                .into_iter()
                .map(|a| a.id)
                .collect()
        };
        assert_eq!(ids(&["INBOX", "UNREAD"]), ["mark-read", "archive"]);
        assert_eq!(ids(&["INBOX"]), ["archive"]);
        assert_eq!(ids(&["UNREAD"]), ["mark-read"]);
        assert_eq!(ids(&[]), Vec::<String>::new());
    }

    #[test]
    fn reads_senders() {
        assert_eq!(sender("\"Alice\" <alice@example.com>"), "Alice");
        assert_eq!(sender("Bob Smith <bob@example.com>"), "Bob Smith");
        assert_eq!(sender("<carol@example.com>"), "carol@example.com");
        assert_eq!(sender("dan@example.com"), "dan@example.com");
    }

    #[test]
    fn titles_messages_without_subjects() {
        let mut message = message(&[]);
        message.payload.headers.retain(|h| h.name != "Subject");
        assert_eq!(message.into_item("me@example.com").title, "(No subject)");
    }
}
