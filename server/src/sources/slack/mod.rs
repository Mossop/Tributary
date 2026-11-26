//! Lists Slack messages: saved ones or to-dos. Slack is reached through an
//! MCP gateway (e.g. Runlayer) offering Slack's MCP tools, authenticated with
//! a personal access token, as workspaces may not allow adding a Slack app.

use std::sync::Arc;

use anyhow::{anyhow, bail};
use async_trait::async_trait;
use serde_json::{Value, json};

use super::{
    Field, FieldKind, FieldOption, ItemKind, Source, SourceError, SourceItem, SourceType,
    references,
};
use crate::accounts::{Accounts, SLACK};
use crate::mcp::{self, Mcp, McpError};

pub mod results;

use results::Message;

pub const ICON: &str = "https://slack.com/favicon.ico";

/// The search tool, which sees every channel and DM the user can.
const SEARCH_TOOL: &str = "slack_search_public_and_private";
/// Gives the current user's profile.
const PROFILE_TOOL: &str = "slack_read_user_profile";
/// Results per page, the most the tool returns.
const PAGE_SIZE: u32 = 20;
/// Stops runaway paging.
const MAX_PAGES: usize = 5;

/// What a source shows: `(value, search filter, label)`.
const SHOW: &[(&str, &str, &str)] = &[
    ("saved", "is:saved", "Saved messages"),
    ("todos", "is:todos", "To-dos"),
];

pub fn source_type() -> SourceType {
    SourceType::new(
        "slack",
        "Slack",
        vec![
            Field {
                key: "account",
                label: "Slack account",
                kind: FieldKind::Account,
                required: true,
                placeholder: None,
                help: None,
                provider: Some(SLACK),
            },
            Field {
                key: "show",
                label: "Show",
                kind: FieldKind::Select,
                required: true,
                placeholder: None,
                help: None,
                provider: None,
            },
        ],
        |config, context| {
            let account = config["account"].as_str().unwrap_or_default().to_string();
            let filter = config["show"]
                .as_str()
                .and_then(|show| SHOW.iter().find(|(value, _, _)| *value == show))
                .map(|(_, filter, _)| *filter)
                .ok_or_else(|| anyhow!("Choose what to show"))?;
            Ok(Arc::new(SlackSource {
                accounts: context.accounts.clone(),
                account,
                filter,
            }))
        },
    )
    .with_icon(ICON)
    .with_options(|_, _, _| {
        Box::pin(async {
            Ok(SHOW
                .iter()
                .map(|(value, _, label)| FieldOption {
                    value: value.to_string(),
                    label: label.to_string(),
                })
                .collect())
        })
    })
}

pub fn http() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder().user_agent("tributary").build()
}

/// A connection to the gateway at `url`, and the names of its tools.
async fn connect(url: &str, token: &str) -> Result<(Mcp, Vec<String>), McpError> {
    let mcp = Mcp::connect(http()?, url, token).await?;
    let names = mcp.tool_names().await?;
    Ok((mcp, names))
}

/// The name of the tool `base` among `names`, or an error saying the
/// gateway lacks `what`.
fn tool(names: &[String], base: &str, what: &str) -> Result<String, McpError> {
    mcp::find_tool(names, base)
        .map(str::to_string)
        .ok_or_else(|| McpError::Api(format!("the MCP server doesn't offer Slack {what}")))
}

/// Checks a gateway and token, returning the Slack user's handle.
pub async fn current_user(url: &str, token: &str) -> Result<String, McpError> {
    let (mcp, names) = connect(url, token).await?;
    tool(&names, SEARCH_TOOL, "search")?;
    let profile = tool(&names, PROFILE_TOOL, "profiles")?;
    let output = mcp
        .call_tool(&profile, json!({ "response_format": "concise" }))
        .await?;
    handle(&output).ok_or_else(|| McpError::Unexpected("no user in the profile".to_string()))
}

/// The handle in a concise profile, whose first line is `handle (Name)`.
fn handle(output: &str) -> Option<String> {
    let text = serde_json::from_str::<Value>(output)
        .ok()
        .and_then(|json| json["result"].as_str().map(str::to_string))
        .unwrap_or_else(|| output.to_string());
    let first = text.lines().next()?.trim();
    let handle = first.split_once(" (").map_or(first, |(handle, _)| handle);
    (!handle.is_empty()).then(|| handle.to_string())
}

pub struct SlackSource {
    accounts: Arc<Accounts>,
    account: String,
    filter: &'static str,
}

impl SlackSource {
    async fn messages(&self, url: &str, token: &str) -> Result<Vec<Message>, McpError> {
        let (mcp, names) = connect(url, token).await?;
        let search = tool(&names, SEARCH_TOOL, "search")?;
        let mut messages = Vec::new();
        let mut cursor = None;
        for _ in 0..MAX_PAGES {
            let mut arguments = json!({
                "filters": self.filter,
                "natural_language_query": "",
                "sort": "timestamp",
                "limit": PAGE_SIZE,
                "include_context": false,
            });
            if let Some(cursor) = cursor {
                arguments["cursor"] = json!(cursor);
            }
            let page = results::parse_page(&mcp.call_tool(&search, arguments).await?);
            messages.extend(page.messages);
            cursor = page.cursor;
            if cursor.is_none() {
                break;
            }
        }
        Ok(messages)
    }

    async fn items(&self) -> anyhow::Result<Vec<SourceItem>> {
        let (account, token) = self.accounts.token(&self.account, SLACK, "Slack").await?;
        let Some(url) = account.api_url.as_deref() else {
            bail!("Slack account {} has no MCP server URL", account.name);
        };
        match self.messages(url, &token).await {
            Ok(messages) => Ok(messages.iter().map(item).collect()),
            Err(McpError::Rejected) => {
                self.accounts.mark_revoked(&self.account);
                bail!(
                    "Slack account {} needs reconnecting in settings",
                    account.name
                )
            }
            Err(error) => bail!("Slack search failed: {error}"),
        }
    }
}

/// The longest title, in characters; longer ones are cut.
const TITLE_LENGTH: usize = 200;

fn item(message: &Message) -> SourceItem {
    let text = results::plain_text(&message.text);
    let first_line = text.lines().map(str::trim).find(|l| !l.is_empty());
    let title = match first_line {
        Some(line) if line.chars().count() > TITLE_LENGTH => {
            format!("{}…", line.chars().take(TITLE_LENGTH).collect::<String>())
        }
        Some(line) => line.to_string(),
        None => format!("Message in {}", message.channel),
    };
    let description = match &message.from {
        Some(from) => format!("{} · from {from}", message.channel),
        None => message.channel.clone(),
    };
    SourceItem {
        id: format!("{}/{}", message.channel_id, message.ts),
        kind: ItemKind::Task,
        title,
        url: message.permalink.clone(),
        icon: None,
        modified: message.time,
        created: Some(message.time),
        description: Some(description),
        due: None,
        actions: Vec::new(),
        due_from: None,
        // The raw text, which has the URLs that links hide.
        references: references::in_text(&message.text),
    }
}

#[async_trait]
impl Source for SlackSource {
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
    use chrono::DateTime;

    use super::*;

    #[test]
    fn reads_the_handle_from_a_profile() {
        assert_eq!(
            handle(r#"{"result":"alice (Alice Example)\nExample Org\n"}"#).as_deref(),
            Some("alice")
        );
        assert_eq!(handle("bob\n").as_deref(), Some("bob"));
        assert_eq!(handle(r#"{"result":""}"#), None);
    }

    #[test]
    fn maps_messages_to_items() {
        let message = Message {
            channel_id: "C1".into(),
            channel: "#general".into(),
            from: Some("Alice".into()),
            ts: "1795694773.642799".into(),
            time: DateTime::from_timestamp(1_795_694_773, 0).unwrap(),
            permalink: "https://example.slack.com/archives/C1/p1795694773642799".into(),
            text: "\nSee <https://bugzilla.mozilla.org/show_bug.cgi?id=123|the bug>\nmore".into(),
        };
        let item = item(&message);
        assert_eq!(item.id, "C1/1795694773.642799");
        assert_eq!(item.title, "See the bug");
        assert_eq!(item.description.as_deref(), Some("#general · from Alice"));
        assert_eq!(item.url, message.permalink);
        assert_eq!(item.created, Some(message.time));
        assert_eq!(item.references, ["bugzilla:123"]);

        let empty = Message {
            text: String::new(),
            from: None,
            ..message
        };
        let item = super::item(&empty);
        assert_eq!(item.title, "Message in #general");
        assert_eq!(item.description.as_deref(), Some("#general"));
    }
}
