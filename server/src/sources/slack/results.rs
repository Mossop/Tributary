//! Parses the text results of the Slack MCP server's search tool.
//!
//! The tool returns JSON whose `results` is markdown meant for an LLM, with a
//! block of labelled lines per message:
//!
//! ```text
//! ### Result 1 of 2
//! Channel: #name (ID: C123)
//! From: Name <email> (ID: U123)
//! Time: 2026-11-26 12:06:13 GMT
//! Message_ts: 1795694773.642799
//! Permalink: [link](https://example.slack.com/archives/C123/p1795694773642799)
//! Text:
//! > the message, in Slack's mrkdwn
//! ```

use std::sync::LazyLock;

use chrono::{DateTime, Utc};
use regex::Regex;
use serde_json::Value;
use tracing::warn;

/// A message found by a search.
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub channel_id: String,
    /// `#name`, or `DM` for direct messages.
    pub channel: String,
    /// The sender's name, if it has one (bots may not).
    pub from: Option<String>,
    pub ts: String,
    pub time: DateTime<Utc>,
    pub permalink: String,
    /// In Slack's mrkdwn.
    pub text: String,
}

/// One page of search results.
#[derive(Debug, Default, PartialEq)]
pub struct Page {
    pub messages: Vec<Message>,
    /// For the next page, if there is one.
    pub cursor: Option<String>,
}

/// Parses a page of the search tool's output.
pub fn parse_page(output: &str) -> Page {
    let (results, pagination) = match serde_json::from_str::<Value>(output) {
        Ok(json) => (
            json["results"].as_str().unwrap_or_default().to_string(),
            json["pagination_info"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        ),
        Err(_) => (output.to_string(), String::new()),
    };
    let messages = results
        .split("\n### Result ")
        .skip(1)
        .filter_map(|block| {
            let message = parse_message(block);
            if message.is_none() {
                warn!(block, "skipping a Slack search result that didn't parse");
            }
            message
        })
        .collect();
    Page {
        messages,
        cursor: cursor(&pagination),
    }
}

/// The value of the line starting `label: `.
fn field<'a>(block: &'a str, label: &str) -> Option<&'a str> {
    block
        .lines()
        .find_map(|line| line.strip_prefix(label)?.strip_prefix(':'))
        .map(str::trim)
}

/// Splits `name (ID: X123)` into the name and id.
fn with_id(value: &str) -> Option<(&str, &str)> {
    let (name, rest) = value.rsplit_once("(ID: ")?;
    let id = rest.split_once(')')?.0;
    Some((name.trim(), id.trim()))
}

fn parse_message(block: &str) -> Option<Message> {
    let (channel, channel_id) = with_id(field(block, "Channel")?)?;
    let from = field(block, "From").and_then(with_id).map(|(name, _)| {
        // `Name <email>`, or empty for some bots.
        name.split_once(" <").map_or(name, |(name, _)| name).trim()
    });
    let ts = field(block, "Message_ts")?;
    let time = parse_ts(ts)?;
    let permalink = field(block, "Permalink")?;
    let permalink = permalink.split_once("](")?.1.strip_suffix(')')?;
    let text: Vec<&str> = block
        .lines()
        .skip_while(|line| !line.starts_with("Text:"))
        .skip(1)
        .take_while(|line| line.trim() != "---")
        .filter_map(|line| line.strip_prefix('>'))
        .map(|line| line.strip_prefix(' ').unwrap_or(line))
        .collect();
    Some(Message {
        channel_id: channel_id.to_string(),
        channel: channel.to_string(),
        from: from.filter(|f| !f.is_empty()).map(str::to_string),
        ts: ts.to_string(),
        time,
        permalink: permalink.to_string(),
        text: text.join("\n").trim().to_string(),
    })
}

/// A message timestamp, e.g. `1795694773.642799` (Unix seconds).
fn parse_ts(ts: &str) -> Option<DateTime<Utc>> {
    let (seconds, micros) = ts.split_once('.').unwrap_or((ts, "0"));
    let micros: u32 = format!("{micros:0<6}").get(..6)?.parse().ok()?;
    DateTime::from_timestamp(seconds.parse().ok()?, micros * 1000)
}

/// The cursor in `For the next page of results use cursor `abc=``.
fn cursor(pagination: &str) -> Option<String> {
    let (_, rest) = pagination.split_once("cursor `")?;
    let cursor = rest.split_once('`')?.0;
    (!cursor.is_empty()).then(|| cursor.to_string())
}

static MRKDWN_LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<([^<>|]+)(?:\|([^<>]*))?>").unwrap());

/// Slack's mrkdwn as plain text: links become their labels (or URLs), and
/// mentions become `@name` or `#channel`.
pub fn plain_text(mrkdwn: &str) -> String {
    let text = MRKDWN_LINK.replace_all(mrkdwn, |c: &regex::Captures| {
        let target = &c[1];
        let label = c.get(2).map(|l| l.as_str()).filter(|l| !l.is_empty());
        match (target.chars().next(), label) {
            (Some('@'), Some(label)) => format!("@{label}"),
            (Some('#'), Some(label)) => format!("#{label}"),
            (Some('!'), Some(label)) => label.to_string(),
            (Some('!'), None) => format!("@{}", &target[1..]),
            (_, Some(label)) => label.to_string(),
            (_, None) => target.to_string(),
        }
    });
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r###"{"results":"# Search Results for: \n\n## Messages (2 results)\n### Result 1 of 2\nChannel: #general (ID: C0507L3TTNG)\nFrom: Alice Example <alice@example.com> (ID: U07QB2ZAF5W) \nTime: 2026-11-26 12:06:13 GMT\nMessage_ts: 1795694773.642799\nPermalink: [link](https:\/\/example.slack.com\/archives\/C0507L3TTNG\/p1795694773642799?thread_ts=1795549932.201239&cid=C0507L3TTNG)\nText: \n> <@UAR1PLD62|Bob> see <https:\/\/bugzilla.mozilla.org\/show_bug.cgi?id=123|the bug>\n> second line\n\n---\n\n### Result 2 of 2\nChannel: DM (ID: D07Q43DARLL)\nParticipants: Me (ID: U4CBS6FUY), Carol (ID: U4D114LBB)\nFrom:  (ID: U00)  [BOT]\nTime: 2026-11-25 09:59:24 GMT\nMessage_ts: 1795600764.183719\nPermalink: [link](https:\/\/example.slack.com\/archives\/D07Q43DARLL\/p1795600764183719)\nText: \n> <https:\/\/docs.google.com\/document\/d\/1|A document>\n\n---\n\n","pagination_info":"For the next page of results use cursor `Q1VSUkVOVF9QQUdFOjI=`\n"}"###;

    #[test]
    fn parses_messages_and_the_cursor() {
        let page = parse_page(PAGE);
        assert_eq!(page.cursor.as_deref(), Some("Q1VSUkVOVF9QQUdFOjI="));
        assert_eq!(page.messages.len(), 2);

        let first = &page.messages[0];
        assert_eq!(first.channel_id, "C0507L3TTNG");
        assert_eq!(first.channel, "#general");
        assert_eq!(first.from.as_deref(), Some("Alice Example"));
        assert_eq!(first.ts, "1795694773.642799");
        assert_eq!(first.time.to_rfc3339(), "2026-11-26T12:06:13.642799+00:00");
        assert_eq!(
            first.permalink,
            "https://example.slack.com/archives/C0507L3TTNG/p1795694773642799?thread_ts=1795549932.201239&cid=C0507L3TTNG"
        );
        assert_eq!(
            first.text,
            "<@UAR1PLD62|Bob> see <https://bugzilla.mozilla.org/show_bug.cgi?id=123|the bug>\nsecond line"
        );

        let dm = &page.messages[1];
        assert_eq!(dm.channel, "DM");
        assert_eq!(dm.channel_id, "D07Q43DARLL");
        assert_eq!(dm.from, None);
    }

    #[test]
    fn parses_the_last_page() {
        let page = parse_page(
            r###"{"results":"# Search Results for: \n\nNo results found.\n","pagination_info":"End of results - No more pages available.\\n"}"###,
        );
        assert_eq!(page, Page::default());
    }

    #[test]
    fn skips_results_that_dont_parse() {
        let page = parse_page(
            r###"{"results":"## Messages\n### Result 1 of 1\nChannel: #general (ID: C1)\nText: \n> no timestamp or link\n","pagination_info":""}"###,
        );
        assert_eq!(page.messages, []);
    }

    #[test]
    fn converts_mrkdwn_to_plain_text() {
        assert_eq!(
            plain_text(
                "<@U1|Bob> in <#C1|general>: <https://example.com|a link> &amp; <https://example.org> <!here>"
            ),
            "@Bob in #general: a link & https://example.org @here"
        );
        assert_eq!(plain_text("1 &lt; 2"), "1 < 2");
    }
}
