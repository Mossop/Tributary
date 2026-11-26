//! GitHub accounts and the GitHub search source, against a mocked GitHub.

mod common;

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use common::{TestApp, start};

/// Mocks `/user` for `token`, as user `alice`.
async fn mock_user(github: &MockServer, token: &str) {
    Mock::given(method("GET"))
        .and(path("/user"))
        .and(header("authorization", format!("Bearer {token}").as_str()))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "id": 42, "login": "alice" })),
        )
        .mount(github)
        .await;
}

async fn add_account(app: &TestApp, github: &MockServer, token: &str) -> (StatusCode, Value) {
    app.request(
        Method::POST,
        "/api/accounts/github",
        Some(json!({ "token": token, "api_url": github.uri() })),
    )
    .await
}

/// Adds an account for `secret` and a source searching with it.
async fn add_source(app: &TestApp, github: &MockServer, query: &str) -> String {
    add_source_with(app, github, query, Value::Null).await
}

/// Adds an account for `secret` and a source searching with it, with
/// `default_due`.
async fn add_source_with(
    app: &TestApp,
    github: &MockServer,
    query: &str,
    default_due: Value,
) -> String {
    mock_user(github, "secret").await;
    let (status, account) = add_account(app, github, "secret").await;
    assert_eq!(status, StatusCode::CREATED, "{account}");
    let (status, source) = app
        .request(
            Method::POST,
            "/api/sources",
            Some(json!({
                "type": "github",
                "name": "PRs",
                "contexts": ["default"],
                "config": { "account": account["id"], "query": query },
                "default_due": default_due,
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{source}");
    source["id"].as_str().unwrap().to_string()
}

async fn last_error(app: &TestApp) -> Value {
    app.get("/api/sources").await[0]["status"]["last_error"].clone()
}

#[tokio::test]
async fn adds_accounts_from_tokens() {
    let github = MockServer::start().await;
    let app = start().await;
    mock_user(&github, "secret").await;
    mock_user(&github, "newer").await;

    let (status, body) = add_account(&app, &github, " secret ").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = body["id"].as_str().unwrap().to_string();
    assert_eq!(
        app.get("/api/accounts").await,
        json!([{
            "id": id,
            "provider": "github",
            "name": "alice",
            "api_url": github.uri(),
            "missing_scopes": false,
            "revoked": false,
        }])
    );

    // A new token for the same user updates the account.
    let (status, body) = add_account(&app, &github, "newer").await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["id"], id);
    assert_eq!(app.get("/api/accounts").await.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn rejects_bad_tokens() {
    let github = MockServer::start().await;
    let app = start().await;
    Mock::given(method("GET"))
        .and(path("/user"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "message": "Bad credentials",
        })))
        .mount(&github)
        .await;

    let (status, body) = add_account(&app, &github, "wrong").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        body["error"],
        "GitHub didn't accept the token; check it hasn't expired"
    );
    let (status, body) = add_account(&app, &github, " ").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "Token is required");
    assert_eq!(app.get("/api/accounts").await, json!([]));
}

#[tokio::test]
async fn lists_github_as_a_token_provider() {
    let app = start().await;
    let providers = app.get("/api/account-providers").await;
    assert_eq!(providers[0]["id"], "github");
    assert_eq!(providers[0]["connect"], "token");
    assert_eq!(providers[0]["configured"], true);
    assert_eq!(providers[1]["connect"], "oauth");
}

#[tokio::test]
async fn searches_and_maps_items() {
    let github = MockServer::start().await;
    let app = start().await;
    Mock::given(method("GET"))
        .and(path("/search/issues"))
        .and(query_param("q", "is:open author:@me"))
        .and(query_param("per_page", "100"))
        .and(header("authorization", "Bearer secret"))
        .and(header("accept", "application/vnd.github+json"))
        .and(header("x-github-api-version", "2022-11-28"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "total_count": 2,
            "incomplete_results": false,
            "items": [
                {
                    "number": 12,
                    "title": "Fix the thing",
                    "html_url": "https://github.com/owner/repo/pull/12",
                    "repository_url": "https://api.github.com/repos/owner/repo",
                    "updated_at": "2026-03-04T05:06:07Z",
                    "user": { "login": "alice" },
                    "state": "open",
                    "draft": false,
                    "pull_request": {
                        "url": "https://api.github.com/repos/owner/repo/pulls/12",
                        "merged_at": null,
                    },
                },
                {
                    "number": 3,
                    "title": "Broken",
                    "html_url": "https://github.com/owner/other/issues/3",
                    "repository_url": "https://api.github.com/repos/owner/other",
                    "updated_at": "2026-02-01T00:00:00Z",
                    "user": null,
                    "state": "closed",
                    "state_reason": "completed",
                },
            ],
        })))
        .mount(&github)
        .await;

    add_source(&app, &github, "is:open author:@me").await;
    app.refresh("default").await;
    assert_eq!(last_error(&app).await, Value::Null);
    assert_eq!(
        app.get("/api/sources").await[0]["icon"],
        "https://github.com/favicon.ico"
    );

    // Without a default due date, timelines aren't fetched.
    let timelines = github
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path().ends_with("/timeline"))
        .count();
    assert_eq!(timelines, 0);

    let items = app.get("/api/contexts/default/items").await;
    // Oldest first.
    let (issue, pr) = (&items[0], &items[1]);
    assert_eq!(pr["id"], "owner/repo#12");
    assert_eq!(pr["kind"], "patch");
    assert_eq!(pr["title"], "Fix the thing");
    assert_eq!(pr["url"], "https://github.com/owner/repo/pull/12");
    assert_eq!(pr["modified"], "2026-03-04T05:06:07Z");
    assert_eq!(pr["description"], "owner/repo #12 · PR by alice");
    assert_eq!(pr["due"], Value::Null);
    assert!(
        pr["icon"]
            .as_str()
            .unwrap()
            .starts_with("data:image/svg+xml,%3Csvg fill='%231a7f37' ")
    );

    assert_eq!(pr["references"], json!(["github:owner/repo:12"]));
    assert_eq!(issue["id"], "owner/other#3");
    assert_eq!(issue["references"], json!(["github:owner/other:3"]));
    assert_eq!(issue["kind"], "issue");
    assert_eq!(issue["description"], "owner/other #3 · Issue");
    assert!(
        issue["icon"]
            .as_str()
            .unwrap()
            .starts_with("data:image/svg+xml,%3Csvg fill='%238250df' ")
    );
}

#[tokio::test]
async fn reports_api_errors() {
    let github = MockServer::start().await;
    let app = start().await;
    Mock::given(method("GET"))
        .and(path("/search/issues"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "message": "API rate limit exceeded",
        })))
        .mount(&github)
        .await;

    add_source(&app, &github, "is:open").await;
    app.refresh("default").await;
    assert_eq!(
        last_error(&app).await,
        "GitHub returned 403 Forbidden: API rate limit exceeded"
    );
}

#[tokio::test]
async fn reports_malformed_responses() {
    let github = MockServer::start().await;
    let app = start().await;
    Mock::given(method("GET"))
        .and(path("/search/issues"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
        .mount(&github)
        .await;

    add_source(&app, &github, "is:open").await;
    app.refresh("default").await;
    assert!(
        last_error(&app)
            .await
            .as_str()
            .unwrap()
            .starts_with("unexpected response from GitHub")
    );
}

#[tokio::test]
async fn rejected_tokens_need_reconnecting() {
    let github = MockServer::start().await;
    let app = start().await;
    Mock::given(method("GET"))
        .and(path("/search/issues"))
        .and(header("authorization", "Bearer secret"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "message": "Bad credentials",
        })))
        .mount(&github)
        .await;
    Mock::given(method("GET"))
        .and(path("/search/issues"))
        .and(header("authorization", "Bearer newer"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "items": [] })))
        .mount(&github)
        .await;

    add_source(&app, &github, "is:open").await;
    app.refresh("default").await;
    assert_eq!(
        last_error(&app).await,
        "GitHub account alice needs reconnecting in settings"
    );
    assert_eq!(app.get("/api/accounts").await[0]["revoked"], true);

    // A new token clears it and refreshes the source.
    mock_user(&github, "newer").await;
    add_account(&app, &github, "newer").await;
    assert_eq!(app.get("/api/accounts").await[0]["revoked"], false);
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while last_error(&app).await != Value::Null {
        assert!(tokio::time::Instant::now() < deadline, "timed out waiting");
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}

#[tokio::test]
async fn sources_need_a_github_account() {
    let app = start().await;
    let (status, body) = app
        .request(
            Method::POST,
            "/api/sources",
            Some(json!({
                "type": "github",
                "name": "PRs",
                "contexts": ["default"],
                "config": { "account": "missing", "query": "is:pr" },
            })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "unknown account `missing`");
}

#[tokio::test]
async fn reviews_are_due_after_they_were_requested() {
    let github = MockServer::start().await;
    let app = start().await;
    Mock::given(method("GET"))
        .and(path("/search/issues"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "items": [
            {
                "number": 12,
                "title": "Fix the thing",
                "html_url": "https://github.com/owner/repo/pull/12",
                "repository_url": "https://api.github.com/repos/owner/repo",
                "updated_at": "2026-03-04T05:06:07Z",
                "user": { "login": "bob" },
                "state": "open",
                "pull_request": { "merged_at": null },
            },
            {
                "number": 3,
                "title": "Broken",
                "html_url": "https://github.com/owner/repo/issues/3",
                "repository_url": "https://api.github.com/repos/owner/repo",
                "updated_at": "2026-03-01T00:00:00Z",
                "user": null,
                "state": "open",
            },
        ]})))
        .mount(&github)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo/issues/12/timeline"))
        .and(header("authorization", "Bearer secret"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "event": "committed" },
            {
                "event": "review_requested",
                "created_at": "2026-03-02T10:00:00Z",
                "requested_reviewer": { "login": "alice" },
            },
        ])))
        .mount(&github)
        .await;

    add_source_with(
        &app,
        &github,
        "is:open",
        json!({ "days": 2, "business_days": false, "from": "requested" }),
    )
    .await;
    app.refresh("default").await;
    assert_eq!(last_error(&app).await, Value::Null);
    let items = app.get("/api/contexts/default/items").await;
    let pr = items
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == "owner/repo#12")
        .unwrap();
    assert_eq!(pr["due"], "2026-03-04T10:00:00Z");
    // Issues have no review to count from.
    let issue = items
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == "owner/repo#3")
        .unwrap();
    assert_eq!(issue["due"], Value::Null);
}

#[tokio::test]
async fn items_can_be_due_after_they_were_created() {
    let github = MockServer::start().await;
    let app = start().await;
    Mock::given(method("GET"))
        .and(path("/search/issues"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "items": [{
            "number": 12,
            "title": "Fix the thing",
            "html_url": "https://github.com/owner/repo/pull/12",
            "repository_url": "https://api.github.com/repos/owner/repo",
            "created_at": "2026-02-27T08:00:00Z",
            "updated_at": "2026-03-04T05:06:07Z",
            "user": { "login": "bob" },
            "state": "open",
            "pull_request": { "merged_at": null },
        }]})))
        .mount(&github)
        .await;
    // Review requests aren't needed.
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo/issues/12/timeline"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(0)
        .mount(&github)
        .await;

    add_source_with(
        &app,
        &github,
        "is:open",
        json!({ "days": 1, "business_days": true, "from": "created" }),
    )
    .await;
    app.refresh("default").await;
    assert_eq!(last_error(&app).await, Value::Null);
    let items = app.get("/api/contexts/default/items").await;
    // Friday + 1 business day.
    assert_eq!(items[0]["due"], "2026-03-02T08:00:00Z");
}
