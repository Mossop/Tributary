//! A minimal MCP client over Streamable HTTP, authenticated with a bearer
//! token: enough to list and call a server's tools.

use std::sync::atomic::{AtomicU64, Ordering};

use reqwest::StatusCode;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::{Value, json};

const PROTOCOL_VERSION: &str = "2025-06-18";
const SESSION_HEADER: &str = "mcp-session-id";

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    /// The token is invalid or revoked.
    #[error("the MCP server didn't accept the token")]
    Rejected,
    #[error("{0}")]
    Api(String),
    #[error("unexpected response from the MCP server: {0}")]
    Unexpected(String),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
}

pub struct Mcp {
    http: reqwest::Client,
    url: String,
    token: String,
    /// Set if the server keeps sessions.
    session: Option<String>,
    next_id: AtomicU64,
}

impl Mcp {
    /// Opens a session with the server at `url`.
    pub async fn connect(http: reqwest::Client, url: &str, token: &str) -> Result<Self, McpError> {
        let mut mcp = Self {
            http,
            url: url.to_string(),
            token: token.to_string(),
            session: None,
            next_id: AtomicU64::new(1),
        };
        let params = json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": { "name": "tributary", "version": env!("CARGO_PKG_VERSION") },
        });
        let (_, session) = mcp.request("initialize", params).await?;
        mcp.session = session;
        mcp.notify("notifications/initialized").await?;
        Ok(mcp)
    }

    fn post(&self, body: &Value) -> reqwest::RequestBuilder {
        let request = self
            .http
            .post(&self.url)
            .bearer_auth(&self.token)
            .header(ACCEPT, "application/json, text/event-stream")
            .json(body);
        match &self.session {
            Some(session) => request.header(SESSION_HEADER, session),
            None => request,
        }
    }

    /// Sends a request, returning its result and any session id the server
    /// gave.
    async fn request(
        &self,
        method: &str,
        params: Value,
    ) -> Result<(Value, Option<String>), McpError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let body = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let response = self.post(&body).send().await?;
        let status = response.status();
        if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
            return Err(McpError::Rejected);
        }
        let session = response
            .headers()
            .get(SESSION_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let event_stream = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("text/event-stream"));
        let text = response.text().await?;
        if !status.is_success() {
            return Err(McpError::Api(format!("the MCP server returned {status}")));
        }
        let message = if event_stream {
            event_stream_response(&text, id)?
        } else {
            serde_json::from_str(&text).map_err(|e| McpError::Unexpected(e.to_string()))?
        };
        if let Some(error) = message.get("error") {
            let text = error["message"].as_str().unwrap_or("unknown error");
            return Err(McpError::Api(text.to_string()));
        }
        Ok((message["result"].clone(), session))
    }

    async fn notify(&self, method: &str) -> Result<(), McpError> {
        let body = json!({ "jsonrpc": "2.0", "method": method });
        let response = self.post(&body).send().await?;
        if matches!(
            response.status(),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ) {
            return Err(McpError::Rejected);
        }
        Ok(())
    }

    /// The names of the server's tools.
    pub async fn tool_names(&self) -> Result<Vec<String>, McpError> {
        let (result, _) = self.request("tools/list", json!({})).await?;
        Ok(result["tools"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|tool| tool["name"].as_str().map(str::to_string))
            .collect())
    }

    /// Calls a tool, returning its text content.
    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<String, McpError> {
        let (result, _) = self
            .request(
                "tools/call",
                json!({ "name": name, "arguments": arguments }),
            )
            .await?;
        let text: Vec<&str> = result["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|c| c["type"] == "text")
            .filter_map(|c| c["text"].as_str())
            .collect();
        let text = text.join("\n");
        if result["isError"] == true {
            return Err(McpError::Api(text));
        }
        Ok(text)
    }
}

/// The tool named `base`, which gateways may give a suffix (e.g.
/// `search__1a2b3c`).
pub fn find_tool<'a>(names: &'a [String], base: &str) -> Option<&'a str> {
    names
        .iter()
        .map(String::as_str)
        .find(|name| *name == base || name.strip_prefix(base).is_some_and(|s| s.starts_with("__")))
}

/// The message answering request `id` in a server-sent event stream.
fn event_stream_response(text: &str, id: u64) -> Result<Value, McpError> {
    for event in text.split("\n\n") {
        let data: Vec<&str> = event
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(str::trim_start)
            .collect();
        if data.is_empty() {
            continue;
        }
        if let Ok(message) = serde_json::from_str::<Value>(&data.join("\n"))
            && message["id"] == id
        {
            return Ok(message);
        }
    }
    Err(McpError::Unexpected(
        "the event stream had no response".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_tools_with_gateway_suffixes() {
        let names = vec![
            "search_public".to_string(),
            "search_public_and_private__1a2b".to_string(),
        ];
        assert_eq!(find_tool(&names, "search_public"), Some("search_public"));
        assert_eq!(
            find_tool(&names, "search_public_and_private"),
            Some("search_public_and_private__1a2b")
        );
        assert_eq!(find_tool(&names, "search"), None);
    }

    #[test]
    fn reads_responses_from_event_streams() {
        let stream = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"note\"}\n\n\
                      event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{\"ok\":true}}\n\n";
        assert_eq!(
            event_stream_response(stream, 3).unwrap()["result"]["ok"],
            true
        );
        assert!(event_stream_response(stream, 4).is_err());
    }
}
