//! Google accounts and the Google Tasks source, against a mocked Google.

mod common;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use reqwest::Url;
use serde_json::{Value, json};
use tower::ServiceExt;
use wiremock::matchers::{body_json, body_string_contains, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use common::{TestApp, start, start_with};
use tributary::{accounts, db};

const TASKS_SCOPE: &str = "https://www.googleapis.com/auth/tasks";
const GMAIL_SCOPE: &str = "https://www.googleapis.com/auth/gmail.modify";

async fn google_app(google: &MockServer) -> TestApp {
    let uri = google.uri();
    start_with(
        &format!(
            r#"
            [polling]
            active = "1h"
            idle = "1h"

            [server]
            public_url = "http://dash.test"

            [google]
            client_id = "client"
            client_secret = "shh"
            auth_url = "{uri}/auth"
            token_url = "{uri}/token"
            userinfo_url = "{uri}/userinfo"
            tasks_api_url = "{uri}/tasks/v1"
            gmail_api_url = "{uri}/gmail/v1"
            "#
        ),
        db::connect_in_memory().await.unwrap(),
    )
    .await
}

/// Makes a GET request expecting a redirect, returning its location.
async fn redirect(app: &TestApp, uri: &str) -> String {
    let response = app
        .router
        .clone()
        .oneshot(Request::get(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER, "GET {uri}");
    response.headers()["location"].to_str().unwrap().to_string()
}

fn query(url: &str, key: &str) -> Option<String> {
    let url = Url::parse(url)
        .or_else(|_| Url::parse(&format!("http://local{url}")))
        .unwrap();
    url.query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
}

/// Mocks Google's side of connecting an account.
async fn mock_consent(google: &MockServer, scopes: &str) {
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=authorization_code"))
        .and(body_string_contains("code=the-code"))
        .and(body_string_contains("client_secret=shh"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "first-token",
            "expires_in": 3600,
            "refresh_token": "refresh-me",
            "scope": scopes,
        })))
        .mount(google)
        .await;
    Mock::given(method("GET"))
        .and(path("/userinfo"))
        .and(header("authorization", "Bearer first-token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "sub": "123", "email": "me@example.com" })),
        )
        .mount(google)
        .await;
}

/// Connects an account through the OAuth flow, returning its id.
async fn connect(app: &TestApp, google: &MockServer) -> String {
    mock_consent(google, &format!("openid email {GMAIL_SCOPE} {TASKS_SCOPE}")).await;
    let consent = redirect(app, "/api/accounts/google/connect").await;
    let state = query(&consent, "state").unwrap();
    let back = redirect(
        app,
        &format!("/api/oauth/google/callback?code=the-code&state={state}"),
    )
    .await;
    assert_eq!(back, "/settings");
    let accounts = app.get("/api/accounts").await;
    accounts[0]["id"].as_str().unwrap().to_string()
}

/// Mocks refreshing the access token to `access-token`.
async fn mock_refresh(google: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=refresh_token"))
        .and(body_string_contains("refresh_token=refresh-me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "access-token",
            "expires_in": 3600,
        })))
        .mount(google)
        .await;
}

async fn mock_lists(google: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/tasks/v1/users/@me/lists"))
        .and(header("authorization", "Bearer access-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [
                { "id": "inbox", "title": "My Tasks" },
                { "id": "shop", "title": "Shopping" },
            ],
        })))
        .mount(google)
        .await;
}

fn task(id: &str, title: &str, updated: &str) -> Value {
    json!({
        "id": id,
        "title": title,
        "status": "needsAction",
        "updated": updated,
        "webViewLink": format!("https://tasks.google.com/task/{id}"),
    })
}

/// Two pages of tasks in `inbox` and one in `shop`.
async fn mock_tasks(google: &MockServer) {
    let tasks = |list: &str| path(format!("/tasks/v1/lists/{list}/tasks"));
    Mock::given(method("GET"))
        .and(tasks("inbox"))
        .and(query_param("showCompleted", "false"))
        .and(query_param("pageToken", "page-2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [task("t2", "Second", "2026-01-02T00:00:00Z")],
        })))
        .with_priority(1)
        .mount(google)
        .await;
    Mock::given(method("GET"))
        .and(tasks("inbox"))
        .and(query_param("showCompleted", "false"))
        .and(header("authorization", "Bearer access-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [{
                "id": "t1",
                "title": "First",
                "notes": "Some notes",
                "status": "needsAction",
                "due": "2026-02-01T00:00:00.000Z",
                "updated": "2026-01-01T00:00:00Z",
            }],
            "nextPageToken": "page-2",
        })))
        .mount(google)
        .await;
    Mock::given(method("GET"))
        .and(tasks("shop"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [task("t3", "Milk", "2026-01-03T00:00:00Z")],
        })))
        .mount(google)
        .await;
}

async fn add_tasks_source(app: &TestApp, account: &str, list: &str) -> (StatusCode, Value) {
    app.request(
        Method::POST,
        "/api/sources",
        Some(json!({
            "type": "google-tasks",
            "name": "Tasks",
            "contexts": ["default"],
            "config": { "account": account, "list": list },
        })),
    )
    .await
}

#[tokio::test]
async fn connect_redirects_to_google() {
    let google = MockServer::start().await;
    let app = google_app(&google).await;
    let consent = redirect(&app, "/api/accounts/google/connect").await;
    assert!(consent.starts_with(&format!("{}/auth?", google.uri())));
    assert_eq!(query(&consent, "client_id").as_deref(), Some("client"));
    assert_eq!(
        query(&consent, "redirect_uri").as_deref(),
        Some("http://dash.test/api/oauth/google/callback")
    );
    assert_eq!(
        query(&consent, "scope").as_deref(),
        Some(format!("openid email {GMAIL_SCOPE} {TASKS_SCOPE}").as_str())
    );
    assert!(query(&consent, "state").is_some());
}

#[tokio::test]
async fn connect_explains_missing_configuration() {
    let app = start().await;
    let back = redirect(&app, "/api/accounts/google/connect").await;
    assert!(
        query(&back, "error")
            .unwrap()
            .contains("Google isn't configured")
    );
    let providers = app.get("/api/account-providers").await;
    let google = &providers[1];
    assert_eq!(
        google,
        &json!({
            "id": "google",
            "name": "Google",
            "icon": "https://www.gstatic.com/images/branding/googleg/1x/googleg_standard_color_48dp.png",
            "connect": "oauth",
            "configured": false,
        })
    );
}

#[tokio::test]
async fn connects_and_reconnects_accounts() {
    let google = MockServer::start().await;
    let app = google_app(&google).await;
    let id = connect(&app, &google).await;

    // Secrets and internal details aren't exposed.
    assert_eq!(
        app.get("/api/accounts").await,
        json!([{
            "id": id,
            "provider": "google",
            "name": "me@example.com",
            "api_url": null,
            "missing_scopes": false,
            "revoked": false,
        }])
    );

    // Connecting the same Google account again updates it.
    assert_eq!(connect(&app, &google).await, id);
    assert_eq!(app.get("/api/accounts").await.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn callback_rejects_unknown_state() {
    let google = MockServer::start().await;
    let app = google_app(&google).await;
    let back = redirect(&app, "/api/oauth/google/callback?code=the-code&state=nope").await;
    assert!(back.starts_with("/settings?error="));
    assert!(query(&back, "error").unwrap().contains("expired"));
    assert_eq!(app.get("/api/accounts").await, json!([]));
}

#[tokio::test]
async fn callback_reports_denied_access() {
    let google = MockServer::start().await;
    let app = google_app(&google).await;
    let consent = redirect(&app, "/api/accounts/google/connect").await;
    let state = query(&consent, "state").unwrap();
    let back = redirect(
        &app,
        &format!("/api/oauth/google/callback?error=access_denied&state={state}"),
    )
    .await;
    assert!(query(&back, "error").unwrap().contains("wasn't granted"));
}

#[tokio::test]
async fn flags_accounts_missing_scopes() {
    let google = MockServer::start().await;
    let app = google_app(&google).await;
    accounts::save_account(
        &app.pool,
        &accounts::NewAccount {
            provider: "google",
            subject: "1",
            name: "old@example.com",
            secret: "r",
            scopes: "openid email",
            api_url: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(app.get("/api/accounts").await[0]["missing_scopes"], true);
}

#[tokio::test]
async fn lists_task_lists_as_options() {
    let google = MockServer::start().await;
    let app = google_app(&google).await;
    let id = connect(&app, &google).await;
    mock_refresh(&google).await;
    mock_lists(&google).await;

    let (status, options) = app
        .request(
            Method::POST,
            "/api/source-types/google-tasks/fields/list/options",
            Some(json!({ "config": { "account": id } })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{options}");
    assert_eq!(
        options,
        json!([
            { "value": "*", "label": "All lists" },
            { "value": "inbox", "label": "My Tasks" },
            { "value": "shop", "label": "Shopping" },
        ])
    );

    // No account chosen yet.
    let (_, options) = app
        .request(
            Method::POST,
            "/api/source-types/google-tasks/fields/list/options",
            Some(json!({ "config": {} })),
        )
        .await;
    assert_eq!(options, json!([]));

    let (status, _) = app
        .request(
            Method::POST,
            "/api/source-types/google-tasks/fields/account/options",
            Some(json!({ "config": {} })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

async fn item_ids(app: &TestApp) -> Vec<String> {
    let items = app.get("/api/contexts/default/items").await;
    items
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_str().unwrap().to_string())
        .collect()
}

async fn item_titles(app: &TestApp) -> Vec<(String, Value)> {
    let items = app.get("/api/contexts/default/items").await;
    items
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["title"].as_str().unwrap().to_string(),
                i["description"].clone(),
            )
        })
        .collect()
}

#[tokio::test]
async fn fetches_tasks_from_one_list() {
    let google = MockServer::start().await;
    let app = google_app(&google).await;
    let id = connect(&app, &google).await;
    mock_refresh(&google).await;
    mock_tasks(&google).await;

    let (status, body) = add_tasks_source(&app, &id, "inbox").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    app.refresh("default").await;

    let items = app.get("/api/contexts/default/items").await;
    assert_eq!(items[0]["title"], "First");
    assert_eq!(items[0]["id"], "inbox/t1");
    assert_eq!(items[0]["actions"][0]["id"], "complete");
    assert_eq!(items[0]["description"], "Some notes");
    assert_eq!(items[0]["due"], "2026-02-01T12:00:00Z");
    // Without a link, the item links to Google Tasks itself.
    assert_eq!(items[0]["url"], "https://tasks.google.com/");
    assert_eq!(items[1]["title"], "Second");
    assert_eq!(items[1]["url"], "https://tasks.google.com/task/t2");
    assert_eq!(items.as_array().unwrap().len(), 2);

    // The source fetched when added and again on refresh, with one access
    // token.
    app.refresh("default").await;
    let refreshes = google
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| String::from_utf8_lossy(&r.body).contains("grant_type=refresh_token"))
        .count();
    assert_eq!(refreshes, 1);

    let sources = app.get("/api/sources").await;
    assert_eq!(
        sources[0]["icon"],
        "https://www.gstatic.com/images/branding/product/1x/tasks_48dp.png"
    );
}

#[tokio::test]
async fn fetches_tasks_from_all_lists() {
    let google = MockServer::start().await;
    let app = google_app(&google).await;
    let id = connect(&app, &google).await;
    mock_refresh(&google).await;
    mock_lists(&google).await;
    mock_tasks(&google).await;

    add_tasks_source(&app, &id, "*").await;
    app.refresh("default").await;
    assert_eq!(
        item_titles(&app).await,
        [
            ("First".to_string(), json!("My Tasks · Some notes")),
            ("Milk".to_string(), json!("Shopping")),
            ("Second".to_string(), json!("My Tasks")),
        ]
    );
}

#[tokio::test]
async fn revoked_access_asks_for_reconnecting() {
    let google = MockServer::start().await;
    let app = google_app(&google).await;
    let id = connect(&app, &google).await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=refresh_token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": "invalid_grant",
            "error_description": "Token has been expired or revoked.",
        })))
        .mount(&google)
        .await;

    add_tasks_source(&app, &id, "inbox").await;
    app.refresh("default").await;
    let sources = app.get("/api/sources").await;
    assert_eq!(
        sources[0]["status"]["last_error"],
        "Google account me@example.com needs reconnecting in settings"
    );
    assert_eq!(app.get("/api/accounts").await[0]["revoked"], true);
}

#[tokio::test]
async fn validates_the_account() {
    let google = MockServer::start().await;
    let app = google_app(&google).await;
    let (status, body) = add_tasks_source(&app, "missing", "inbox").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "unknown account `missing`");

    let (status, body) = app
        .request(
            Method::POST,
            "/api/sources",
            Some(json!({
                "type": "google-tasks",
                "name": "Tasks",
                "contexts": ["default"],
                "config": { "list": "inbox" },
            })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "Google account is required");
}

#[tokio::test]
async fn accounts_in_use_cant_be_removed() {
    let google = MockServer::start().await;
    let app = google_app(&google).await;
    let id = connect(&app, &google).await;
    mock_refresh(&google).await;
    mock_tasks(&google).await;
    let (_, source) = add_tasks_source(&app, &id, "inbox").await;

    let (status, body) = app
        .request(Method::DELETE, &format!("/api/accounts/{id}"), None)
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        body["error"],
        "these sources use this account, so change or delete them first: Tasks"
    );

    let source = source["id"].as_str().unwrap();
    app.request(Method::DELETE, &format!("/api/sources/{source}"), None)
        .await;
    let (status, _) = app
        .request(Method::DELETE, &format!("/api/accounts/{id}"), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(app.get("/api/accounts").await, json!([]));

    let (status, _) = app
        .request(Method::DELETE, &format!("/api/accounts/{id}"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn reconnecting_refreshes_failing_sources() {
    let google = MockServer::start().await;
    let app = google_app(&google).await;
    let id = connect(&app, &google).await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=refresh_token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({ "error": "invalid_grant" })))
        .mount(&google)
        .await;
    add_tasks_source(&app, &id, "inbox").await;
    let mut events = app.workspace.events.subscribe();
    app.refresh("default").await;
    assert!(item_titles(&app).await.is_empty());
    assert_eq!(app.get("/api/accounts").await[0]["revoked"], true);
    // Clients are told, so they can show that the account needs attention.
    let mut told = false;
    while let Ok(event) = events.try_recv() {
        told |= event == tributary::events::Event::SettingsChanged;
    }
    assert!(told);

    // Access is granted again.
    google.reset().await;
    mock_refresh(&google).await;
    mock_tasks(&google).await;
    connect(&app, &google).await;

    assert_eq!(app.get("/api/accounts").await[0]["revoked"], false);
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while item_titles(&app).await.len() != 2 {
        assert!(tokio::time::Instant::now() < deadline, "timed out waiting");
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}

#[tokio::test]
async fn ids_include_the_list() {
    let google = MockServer::start().await;
    let app = google_app(&google).await;
    let id = connect(&app, &google).await;
    mock_refresh(&google).await;
    mock_lists(&google).await;
    mock_tasks(&google).await;

    add_tasks_source(&app, &id, "*").await;
    app.refresh("default").await;
    assert_eq!(item_ids(&app).await, ["inbox/t1", "shop/t3", "inbox/t2"]);
}

#[tokio::test]
async fn completes_tasks() {
    let google = MockServer::start().await;
    let app = google_app(&google).await;
    let id = connect(&app, &google).await;
    mock_refresh(&google).await;
    mock_tasks(&google).await;
    Mock::given(method("PATCH"))
        .and(path("/tasks/v1/lists/inbox/tasks/t1"))
        .and(header("authorization", "Bearer access-token"))
        .and(body_json(json!({ "status": "completed" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "t1" })))
        .expect(1)
        .mount(&google)
        .await;

    let (_, source) = add_tasks_source(&app, &id, "inbox").await;
    app.refresh("default").await;
    let fetches_before = task_fetches(&google).await;

    let source = source["id"].as_str().unwrap();
    let (status, body) = app
        .request(
            Method::POST,
            &format!("/api/items/{source}/inbox%2Ft1/actions/complete"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    // The source was refreshed after completing the task.
    assert!(task_fetches(&google).await > fetches_before);
}

#[tokio::test]
async fn reports_failures_to_complete_tasks() {
    let google = MockServer::start().await;
    let app = google_app(&google).await;
    let id = connect(&app, &google).await;
    mock_refresh(&google).await;
    mock_tasks(&google).await;
    Mock::given(method("PATCH"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "error": { "message": "Request had insufficient authentication scopes." },
        })))
        .mount(&google)
        .await;

    let (_, source) = add_tasks_source(&app, &id, "inbox").await;
    app.refresh("default").await;
    let source = source["id"].as_str().unwrap();
    let (status, body) = app
        .request(
            Method::POST,
            &format!("/api/items/{source}/inbox%2Ft1/actions/complete"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(
        body["error"],
        "Google Tasks returned 403 Forbidden: Request had insufficient authentication scopes."
    );
}

/// How many times the inbox list's tasks were fetched.
async fn task_fetches(google: &MockServer) -> usize {
    google
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.method.as_str() == "GET" && r.url.path() == "/tasks/v1/lists/inbox/tasks")
        .count()
}

/// Mocks a Gmail search for `is:unread` finding a conversation of two
/// messages, one unread in the inbox (`t1`), and a read, archived one
/// (`t2`).
async fn mock_gmail(google: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/threads"))
        .and(header("authorization", "Bearer access-token"))
        .and(query_param("q", "is:unread"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "threads": [{ "id": "t1" }, { "id": "t2" }],
            "resultSizeEstimate": 2,
        })))
        .mount(google)
        .await;
    let message = |labels: Value, subject: &str| {
        json!({
            "labelIds": labels,
            "snippet": "Hello",
            "internalDate": "1795694773000",
            "payload": { "headers": [
                { "name": "Subject", "value": subject },
                { "name": "From", "value": "Alice <alice@example.com>" },
            ]},
        })
    };
    for (id, messages) in [
        (
            "t1",
            json!([
                message(json!(["INBOX"]), "Please review"),
                message(json!(["INBOX", "UNREAD"]), "Re: Please review"),
            ]),
        ),
        ("t2", json!([message(json!([]), "Old news")])),
    ] {
        Mock::given(method("GET"))
            .and(path(format!("/gmail/v1/users/me/threads/{id}")))
            .and(query_param("format", "metadata"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "id": id, "messages": messages })),
            )
            .mount(google)
            .await;
    }
}

async fn add_gmail_source(app: &TestApp, account: &str) -> Value {
    let (status, source) = app
        .request(
            Method::POST,
            "/api/sources",
            Some(json!({
                "type": "gmail",
                "name": "Mail",
                "contexts": ["default"],
                "config": { "account": account, "query": "is:unread" },
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{source}");
    source
}

#[tokio::test]
async fn lists_gmail_conversations_with_actions() {
    let google = MockServer::start().await;
    let app = google_app(&google).await;
    let id = connect(&app, &google).await;
    mock_refresh(&google).await;
    mock_gmail(&google).await;
    add_gmail_source(&app, &id).await;
    app.refresh("default").await;

    let items = app.get("/api/contexts/default/items").await;
    let items: Vec<(&str, &str, &str, Vec<&str>)> = items
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["id"].as_str().unwrap(),
                i["title"].as_str().unwrap(),
                i["url"].as_str().unwrap(),
                i["actions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|a| a["id"].as_str().unwrap())
                    .collect(),
            )
        })
        .collect();
    assert_eq!(
        items,
        [
            (
                "t1",
                "Please review",
                "https://mail.google.com/mail/?authuser=me%40example.com#all/t1",
                vec!["mark-read", "archive"]
            ),
            (
                "t2",
                "Old news",
                "https://mail.google.com/mail/?authuser=me%40example.com#all/t2",
                vec![]
            ),
        ]
    );
}

#[tokio::test]
async fn marks_conversations_read_and_archives_them() {
    let google = MockServer::start().await;
    let app = google_app(&google).await;
    let id = connect(&app, &google).await;
    mock_refresh(&google).await;
    mock_gmail(&google).await;
    for label in ["UNREAD", "INBOX"] {
        Mock::given(method("POST"))
            .and(path("/gmail/v1/users/me/threads/t1/modify"))
            .and(header("authorization", "Bearer access-token"))
            .and(body_json(json!({ "removeLabelIds": [label] })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "t1" })))
            .expect(1)
            .mount(&google)
            .await;
    }
    let source = add_gmail_source(&app, &id).await;
    app.refresh("default").await;

    let source = source["id"].as_str().unwrap();
    for action in ["mark-read", "archive"] {
        let (status, body) = app
            .request(
                Method::POST,
                &format!("/api/items/{source}/t1/actions/{action}"),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{action}: {body}");
    }
}
