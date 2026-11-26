//! Slack accounts and sources, against a mocked MCP gateway.

mod common;

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use common::{TestApp, start};

const TOKEN: &str = "pat-123";
const SEARCH: &str = "slack_search_public_and_private__abc";
const PROFILE: &str = "slack_read_user_profile__def";

fn rpc(result: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({ "jsonrpc": "2.0", "id": 1, "result": result }))
}

/// A tool result whose text is `text`.
fn tool_text(text: &str) -> ResponseTemplate {
    rpc(json!({ "content": [{ "type": "text", "text": text }], "isError": false }))
}

/// Mocks a gateway's handshake, tools and profile at `/mcp`. Requests after
/// `initialize` must carry its session id.
async fn mock_gateway(gateway: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/mcp"))
        .and(header("authorization", format!("Bearer {TOKEN}").as_str()))
        .and(body_partial_json(json!({ "method": "initialize" })))
        .respond_with(
            rpc(json!({
                "protocolVersion": "2025-06-18",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "Gateway", "version": "1" },
            }))
            .insert_header("mcp-session-id", "session-1"),
        )
        .mount(gateway)
        .await;
    Mock::given(method("POST"))
        .and(path("/mcp"))
        .and(header("mcp-session-id", "session-1"))
        .and(body_partial_json(
            json!({ "method": "notifications/initialized" }),
        ))
        .respond_with(ResponseTemplate::new(202))
        .mount(gateway)
        .await;
    Mock::given(method("POST"))
        .and(path("/mcp"))
        .and(header("mcp-session-id", "session-1"))
        .and(body_partial_json(json!({ "method": "tools/list" })))
        .respond_with(rpc(json!({ "tools": [
            { "name": "slack_search_public__xyz" },
            { "name": SEARCH },
            { "name": PROFILE },
        ]})))
        .mount(gateway)
        .await;
    Mock::given(method("POST"))
        .and(path("/mcp"))
        .and(body_partial_json(
            json!({ "method": "tools/call", "params": { "name": PROFILE } }),
        ))
        .respond_with(tool_text(
            &json!({ "result": "alice (Alice Example)\nExample Org\n" }).to_string(),
        ))
        .mount(gateway)
        .await;
}

async fn add_account(app: &TestApp, gateway: &MockServer, token: &str) -> (StatusCode, Value) {
    app.request(
        Method::POST,
        "/api/accounts/slack",
        Some(json!({ "token": token, "api_url": format!("{}/mcp", gateway.uri()) })),
    )
    .await
}

/// A search result block.
fn result(n: usize, channel: &str, text: &str) -> String {
    let ts = format!("17956947{n:02}.000100");
    let id = ts.replace('.', "");
    format!(
        "### Result {n} of 2\nChannel: {channel} (ID: C{n})\nFrom: Bob <bob@example.com> (ID: U{n})\n\
         Time: 2026-11-26 12:00:00 GMT\nMessage_ts: {ts}\n\
         Permalink: [link](https://example.slack.com/archives/C{n}/p{id})\nText: \n> {text}\n\n---\n\n"
    )
}

fn page(results: &[String], cursor: Option<&str>) -> String {
    let pagination = match cursor {
        Some(cursor) => format!("For the next page of results use cursor `{cursor}`\n"),
        None => "End of results - No more pages available.\n".to_string(),
    };
    json!({
        "results": format!("# Search Results for: \n\n## Messages\n{}", results.concat()),
        "pagination_info": pagination,
    })
    .to_string()
}

#[tokio::test]
async fn adds_accounts_from_tokens() {
    let gateway = MockServer::start().await;
    let app = start().await;
    mock_gateway(&gateway).await;

    let (status, body) = add_account(&app, &gateway, TOKEN).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["provider"], "slack");
    assert_eq!(body["name"], "alice");
    assert_eq!(body["api_url"], format!("{}/mcp", gateway.uri()));
}

#[tokio::test]
async fn rejects_bad_tokens_and_missing_urls() {
    let gateway = MockServer::start().await;
    let app = start().await;
    Mock::given(method("POST"))
        .and(path("/mcp"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&gateway)
        .await;

    let (status, body) = add_account(&app, &gateway, "wrong").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "The MCP server didn't accept the token");

    let (status, body) = app
        .request(
            Method::POST,
            "/api/accounts/slack",
            Some(json!({ "token": TOKEN, "api_url": " " })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "The MCP server URL is required");
    assert_eq!(app.get("/api/accounts").await, json!([]));
}

#[tokio::test]
async fn lists_to_dos_across_pages() {
    let gateway = MockServer::start().await;
    let app = start().await;
    mock_gateway(&gateway).await;
    let search = |arguments: Value| {
        Mock::given(method("POST"))
            .and(path("/mcp"))
            .and(body_partial_json(json!({
                "method": "tools/call",
                "params": { "name": SEARCH, "arguments": arguments },
            })))
    };
    search(json!({ "filters": "is:todos", "sort": "timestamp", "include_context": false }))
        .respond_with(tool_text(&page(
            &[result(
                1,
                "#general",
                "Review <https://bugzilla.mozilla.org/show_bug.cgi?id=42|the bug>",
            )],
            Some("next"),
        )))
        .mount(&gateway)
        .await;
    search(json!({ "filters": "is:todos", "cursor": "next" }))
        .respond_with(tool_text(&page(
            &[result(2, "DM", "Ping <@U9|Carol>")],
            None,
        )))
        .with_priority(1)
        .mount(&gateway)
        .await;

    let (_, account) = add_account(&app, &gateway, TOKEN).await;
    let (status, body) = app
        .request(
            Method::POST,
            "/api/sources",
            Some(json!({
                "type": "slack",
                "name": "Slack",
                "contexts": ["default"],
                "config": { "account": account["id"], "show": "todos" },
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    app.refresh("default").await;
    let sources = app.get("/api/sources").await;
    assert_eq!(sources[0]["status"]["last_error"], Value::Null);

    let items = app.get("/api/contexts/default/items").await;
    let items: Vec<(&str, &str, &str, &str)> = items
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["id"].as_str().unwrap(),
                i["title"].as_str().unwrap(),
                i["description"].as_str().unwrap(),
                i["url"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        items,
        [
            (
                "C2/1795694702.000100",
                "Ping @Carol",
                "DM · from Bob",
                "https://example.slack.com/archives/C2/p1795694702000100"
            ),
            (
                "C1/1795694701.000100",
                "Review the bug",
                "#general · from Bob",
                "https://example.slack.com/archives/C1/p1795694701000100"
            ),
        ]
    );
}

#[tokio::test]
async fn rejected_tokens_need_reconnecting() {
    let gateway = MockServer::start().await;
    let app = start().await;
    mock_gateway(&gateway).await;
    let (_, account) = add_account(&app, &gateway, TOKEN).await;
    app.request(
        Method::POST,
        "/api/sources",
        Some(json!({
            "type": "slack",
            "name": "Slack",
            "contexts": ["default"],
            "config": { "account": account["id"], "show": "saved" },
        })),
    )
    .await;

    // The token is revoked.
    gateway.reset().await;
    Mock::given(method("POST"))
        .and(path("/mcp"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&gateway)
        .await;
    app.refresh("default").await;
    let sources = app.get("/api/sources").await;
    assert_eq!(
        sources[0]["status"]["last_error"],
        "Slack account alice needs reconnecting in settings"
    );
    assert_eq!(app.get("/api/accounts").await[0]["revoked"], true);
}
