//! Lists Gmail conversations matching a search, with quick actions to mark
//! them read and to archive them.

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
/// The most conversations a source lists.
const MAX_THREADS: &str = "100";

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
                help: Some("Any Gmail search. Shows up to 100 conversations, most recent first."),
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
struct ThreadList {
    #[serde(default)]
    threads: Vec<ThreadRef>,
}

#[derive(Deserialize)]
struct ThreadRef {
    id: String,
}

/// A conversation, with its messages oldest first.
#[derive(Deserialize)]
struct Thread {
    id: String,
    #[serde(default)]
    messages: Vec<Message>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Message {
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

    fn time(&self) -> DateTime<chrono::Utc> {
        self.internal_date
            .parse()
            .ok()
            .and_then(DateTime::from_timestamp_millis)
            .unwrap_or_default()
    }

    fn has_label(&self, label: &str) -> bool {
        self.label_ids.iter().any(|l| l == label)
    }
}

impl Thread {
    /// The item for a conversation in `email`'s mailbox, or `None` if it
    /// has no messages.
    fn into_item(self, email: &str) -> Option<SourceItem> {
        let first = self.messages.first()?;
        let last = self.messages.last()?;
        let subject = first
            .header("Subject")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("(No subject)")
            .to_string();
        let snippet = decode_entities(&last.snippet);
        let description = match last.header("From").map(sender) {
            Some(from) if snippet.is_empty() => from,
            Some(from) => format!("{from} · {snippet}"),
            None => snippet.clone(),
        };
        let any = |label| self.messages.iter().any(|m| m.has_label(label));
        let mut actions = Vec::new();
        if any(UNREAD) {
            actions.push(QuickAction {
                id: MARK_READ.to_string(),
                label: "Mark as read".to_string(),
                icon: "symbol:mark_email_read".to_string(),
                url: None,
            });
        }
        if any(INBOX) {
            actions.push(QuickAction {
                id: ARCHIVE.to_string(),
                label: "Archive".to_string(),
                icon: "symbol:archive".to_string(),
                url: None,
            });
        }
        let refs = references::in_text(&format!("{subject}\n{snippet}"));
        let created = first.time();
        let modified = last.time();
        Some(SourceItem {
            url: format!(
                "https://mail.google.com/mail/?authuser={}#all/{}",
                query_value(email),
                self.id
            ),
            id: self.id,
            kind: ItemKind::Task,
            title: subject,
            icon: None,
            modified,
            created: Some(created),
            description: (!description.is_empty()).then_some(description),
            due: None,
            actions,
            due_from: None,
            references: refs,
        })
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
        let list: ThreadList = self
            .get(
                &token,
                "/threads",
                &[("q", self.query.as_str()), ("maxResults", MAX_THREADS)],
            )
            .await?;
        let token = &token;
        let threads: Vec<Thread> = stream::iter(list.threads)
            .map(|thread| async move {
                self.get(
                    token,
                    &format!("/threads/{}", thread.id),
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
        Ok(threads
            .into_iter()
            .filter_map(|t| t.into_item(&account.name))
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
                "{}/users/me/threads/{item_id}/modify",
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

    fn message(labels: &[&str], date: &str, subject: &str, from: &str, snippet: &str) -> Value {
        json!({
            "labelIds": labels,
            "snippet": snippet,
            "internalDate": date,
            "payload": { "headers": [
                { "name": "Subject", "value": subject },
                { "name": "From", "value": from },
            ]},
        })
    }

    fn thread(messages: Vec<Value>) -> Thread {
        serde_json::from_value(json!({ "id": "t1", "messages": messages })).unwrap()
    }

    /// A thread of a single message.
    fn single(labels: &[&str]) -> Thread {
        thread(vec![message(
            labels,
            "1795694773000",
            "Help",
            "Alice <alice@example.com>",
            "Hi",
        )])
    }

    #[test]
    fn maps_conversations() {
        let item = thread(vec![
            message(
                &["UNREAD"],
                "1795694773000",
                "Help with https://bugzilla.mozilla.org/show_bug.cgi?id=42",
                "\"Alice Example\" <alice@example.com>",
                "Can you look?",
            ),
            message(
                &["INBOX"],
                "1795698373000",
                "Re: Help with https://bugzilla.mozilla.org/show_bug.cgi?id=42",
                "Bob <bob@example.com>",
                "It&#39;s urgent &amp; important",
            ),
        ])
        .into_item("me+work@example.com")
        .unwrap();
        assert_eq!(item.id, "t1");
        assert_eq!(item.kind, ItemKind::Task);
        // The first message's subject, and the latest message's sender and
        // snippet.
        assert_eq!(
            item.title,
            "Help with https://bugzilla.mozilla.org/show_bug.cgi?id=42"
        );
        assert_eq!(
            item.description.as_deref(),
            Some("Bob · It's urgent & important")
        );
        assert_eq!(
            item.url,
            "https://mail.google.com/mail/?authuser=me%2Bwork%40example.com#all/t1"
        );
        assert_eq!(item.created.unwrap().timestamp(), 1_795_694_773);
        assert_eq!(item.modified.timestamp(), 1_795_698_373);
        assert_eq!(item.references, ["bugzilla:42"]);
        // Labels on any message count.
        let actions: Vec<_> = item.actions.into_iter().map(|a| a.id).collect();
        assert_eq!(actions, ["mark-read", "archive"]);
    }

    #[test]
    fn offers_actions_by_label() {
        let ids = |labels: &[&str]| -> Vec<String> {
            single(labels)
                .into_item("me@example.com")
                .unwrap()
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
    fn skips_empty_conversations() {
        assert!(thread(vec![]).into_item("me@example.com").is_none());
    }

    #[test]
    fn reads_senders() {
        assert_eq!(sender("\"Alice\" <alice@example.com>"), "Alice");
        assert_eq!(sender("Bob Smith <bob@example.com>"), "Bob Smith");
        assert_eq!(sender("<carol@example.com>"), "carol@example.com");
        assert_eq!(sender("dan@example.com"), "dan@example.com");
    }

    #[test]
    fn titles_conversations_without_subjects() {
        let mut thread = single(&[]);
        thread.messages[0]
            .payload
            .headers
            .retain(|h| h.name != "Subject");
        assert_eq!(
            thread.into_item("me@example.com").unwrap().title,
            "(No subject)"
        );
    }
}
