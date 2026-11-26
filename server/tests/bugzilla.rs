//! Bugzilla accounts and the Bugzilla source, against a mocked Bugzilla.

mod common;

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};
use wiremock::matchers::{header, method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

use common::{TestApp, start};

const ME: &str = "me@example.com";
const KEY: &str = "the-api-key";

async fn mock_whoami(bugzilla: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/rest/whoami"))
        .and(header("x-bugzilla-api-key", KEY))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": 42,
            "name": ME,
            "real_name": "Me",
            "nick": "me",
        })))
        .mount(bugzilla)
        .await;
}

fn invalid_key() -> ResponseTemplate {
    // As BMO sends it, with `error` as a number.
    ResponseTemplate::new(400).set_body_json(json!({
        "code": 306,
        "error": 1,
        "message": "The API key you specified is invalid.",
    }))
}

async fn add_account(app: &TestApp, bugzilla: &MockServer, key: &str) -> (StatusCode, Value) {
    app.request(
        Method::POST,
        "/api/accounts/bugzilla",
        Some(json!({ "token": key, "api_url": bugzilla.uri() })),
    )
    .await
}

async fn add_source(app: &TestApp, bugzilla: &MockServer, show: &str) {
    mock_whoami(bugzilla).await;
    let (status, account) = add_account(app, bugzilla, KEY).await;
    assert_eq!(status, StatusCode::CREATED, "{account}");
    let (status, body) = app
        .request(
            Method::POST,
            "/api/sources",
            Some(json!({
                "type": "bugzilla",
                "name": "Bugs",
                "contexts": ["default"],
                "config": { "account": account["id"], "show": show },
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

fn bug(id: u64, flags: Value) -> Value {
    json!({
        "id": id,
        "summary": format!("Bug summary {id}"),
        "status": "NEW",
        "product": "Firefox",
        "component": "General",
        "last_change_time": format!("2026-01-{:02}T00:00:00Z", id % 28 + 1),
        "flags": flags,
    })
}

fn flag(name: &str, status: &str, requestee: &str, setter: &str, date: &str) -> Value {
    json!({
        "id": 1,
        "name": name,
        "type_id": 800,
        "status": status,
        "setter": setter,
        "requestee": requestee,
        "creation_date": date,
        "modification_date": date,
    })
}

async fn items(app: &TestApp) -> Vec<(String, Value)> {
    let items = app.get("/api/contexts/default/items").await;
    items
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["id"].as_str().unwrap().to_string(),
                i["description"].clone(),
            )
        })
        .collect()
}

async fn last_error(app: &TestApp) -> Value {
    app.get("/api/sources").await[0]["status"]["last_error"].clone()
}

#[tokio::test]
async fn adds_accounts_from_api_keys() {
    let bugzilla = MockServer::start().await;
    let app = start().await;
    mock_whoami(&bugzilla).await;

    let (status, body) = add_account(&app, &bugzilla, &format!(" {KEY} ")).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["provider"], "bugzilla");
    assert_eq!(body["name"], ME);
    assert_eq!(body["api_url"], bugzilla.uri());

    let (_, again) = add_account(&app, &bugzilla, KEY).await;
    assert_eq!(again["id"], body["id"]);
    assert_eq!(app.get("/api/accounts").await.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn rejects_bad_api_keys() {
    let bugzilla = MockServer::start().await;
    let app = start().await;
    Mock::given(path("/rest/whoami"))
        .respond_with(invalid_key())
        .mount(&bugzilla)
        .await;
    let (status, body) = add_account(&app, &bugzilla, "wrong").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "Bugzilla didn't accept the API key");
}

#[tokio::test]
async fn lists_open_bugs_assigned_to_me_across_pages() {
    let bugzilla = MockServer::start().await;
    let app = start().await;
    let first_page: Vec<Value> = (1..=500).map(|id| bug(id, json!([]))).collect();
    Mock::given(path("/rest/bug"))
        .and(header("x-bugzilla-api-key", KEY))
        .and(query_param("f1", "assigned_to"))
        .and(query_param("o1", "equals"))
        .and(query_param("v1", ME))
        .and(query_param("status", "__open__"))
        .and(query_param("order", "bug_id"))
        .and(query_param("limit", "500"))
        .and(query_param("offset", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "bugs": first_page })))
        .mount(&bugzilla)
        .await;
    Mock::given(path("/rest/bug"))
        .and(query_param("f1", "assigned_to"))
        .and(query_param("offset", "500"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "bugs": [bug(501, json!([]))] })),
        )
        .mount(&bugzilla)
        .await;

    add_source(&app, &bugzilla, "assigned").await;
    app.refresh("default").await;
    assert_eq!(last_error(&app).await, Value::Null);
    let items = app.get("/api/contexts/default/items").await;
    assert_eq!(items.as_array().unwrap().len(), 501);
    let item = items
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == "501")
        .unwrap();
    assert_eq!(item["kind"], "issue");
    assert_eq!(item["title"], "Bug summary 501");
    assert_eq!(
        item["url"],
        format!("{}/show_bug.cgi?id=501", bugzilla.uri())
    );
    assert_eq!(item["description"], "Bug 501 · Firefox :: General · NEW");
    assert_eq!(item["references"], json!(["bugzilla:501"]));
    assert_eq!(
        app.get("/api/sources").await[0]["icon"],
        "https://bugzilla.mozilla.org/favicon.ico"
    );
}

#[tokio::test]
async fn lists_bugs_with_a_flag_requested_of_me() {
    let bugzilla = MockServer::start().await;
    let app = start().await;
    Mock::given(path("/rest/bug"))
        .and(query_param("f1", "requestees.login_name"))
        .and(query_param("v1", ME))
        .and(query_param("f2", "flagtypes.name"))
        .and(query_param("v2", "needinfo?"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "bugs": [
            // Needinfo of me.
            bug(1, json!([flag("needinfo", "?", ME, "alice@example.com", "2026-02-03T00:00:00Z")])),
            // Needinfo of someone else; something else of me.
            bug(2, json!([
                flag("needinfo", "?", "bob@example.com", "alice@example.com", "2026-02-03T00:00:00Z"),
                flag("review", "?", ME, "alice@example.com", "2026-02-03T00:00:00Z"),
            ])),
            // Needinfo of me on an attachment.
            bug(3, json!([])),
            // Only on an obsolete attachment.
            bug(4, json!([])),
        ]})))
        .mount(&bugzilla)
        .await;
    let attachments = |bug: u64, obsolete: bool| {
        let flags = json!([flag(
            "needinfo",
            "?",
            ME,
            "carol@example.com",
            "2026-02-05T00:00:00Z"
        )]);
        ResponseTemplate::new(200).set_body_json(json!({
            "bugs": { bug.to_string(): [{ "id": 10, "is_obsolete": i32::from(obsolete), "flags": flags }] },
            "attachments": {},
        }))
    };
    for (bug, obsolete) in [(2, false), (3, false), (4, true)] {
        let response = if bug == 2 {
            // Bug 2's attachments have nothing for me.
            ResponseTemplate::new(200)
                .set_body_json(json!({ "bugs": { "2": [] }, "attachments": {} }))
        } else {
            attachments(bug, obsolete)
        };
        Mock::given(path(format!("/rest/bug/{bug}/attachment")))
            .respond_with(response)
            .mount(&bugzilla)
            .await;
    }

    add_source(&app, &bugzilla, "request:needinfo").await;
    app.refresh("default").await;
    assert_eq!(last_error(&app).await, Value::Null);
    assert_eq!(
        items(&app).await,
        [
            (
                "1".to_string(),
                json!("Bug 1 · needinfo? from alice@example.com")
            ),
            (
                "3".to_string(),
                json!("Bug 3 · needinfo? from carol@example.com (attachment)")
            ),
        ]
    );
    // When I was asked, not when the bug last changed.
    let items = app.get("/api/contexts/default/items").await;
    assert_eq!(items[0]["modified"], "2026-02-03T00:00:00Z");
    assert_eq!(items[1]["modified"], "2026-02-05T00:00:00Z");
}

#[tokio::test]
async fn lists_bugs_with_any_flag_requested_of_me() {
    let bugzilla = MockServer::start().await;
    let app = start().await;
    Mock::given(path("/rest/bug"))
        .and(query_param("f1", "requestees.login_name"))
        .and(query_param_is_missing("f2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "bugs": [
            bug(5, json!([
                flag("needinfo", "?", ME, "alice@example.com", "2026-02-03T00:00:00Z"),
                flag("qe-verify", "?", ME, "bob@example.com", "2026-02-04T00:00:00Z"),
                flag("review", "+", ME, "bob@example.com", "2026-02-04T00:00:00Z"),
            ])),
        ]})))
        .mount(&bugzilla)
        .await;

    add_source(&app, &bugzilla, "request:*").await;
    app.refresh("default").await;
    assert_eq!(
        items(&app).await,
        [(
            "5".to_string(),
            json!("Bug 5 · needinfo? from alice@example.com, qe-verify? from bob@example.com")
        )]
    );
}

#[tokio::test]
async fn offers_flags_recently_requested_of_me() {
    let bugzilla = MockServer::start().await;
    let app = start().await;
    let options_for = |config: Value| {
        let app = &app;
        async move {
            app.request(
                Method::POST,
                "/api/source-types/bugzilla/fields/show/options",
                Some(json!({ "config": config })),
            )
            .await
        }
    };

    // Without an account.
    let (status, options) = options_for(json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        options,
        json!([
            { "value": "assigned", "label": "Bugs assigned to me" },
            { "value": "request:needinfo", "label": "Requests: needinfo" },
            { "value": "request:*", "label": "Requests: any flag" },
        ])
    );

    mock_whoami(&bugzilla).await;
    Mock::given(path(format!("/rest/review/flag_activity/requestee/{ME}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "type": { "name": "needinfo", "is_requesteeble": 1 } },
            { "type": { "name": "review", "is_requesteeble": true } },
            { "type": { "name": "review", "is_requesteeble": true } },
            { "type": { "name": "approval-mozilla-beta", "is_requesteeble": 1 } },
            { "type": { "name": "in-testsuite", "is_requesteeble": 0 } },
        ])))
        .mount(&bugzilla)
        .await;
    let (_, account) = add_account(&app, &bugzilla, KEY).await;
    let (_, options) = options_for(json!({ "account": account["id"] })).await;
    let values: Vec<&str> = options
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["value"].as_str().unwrap())
        .collect();
    assert_eq!(
        values,
        [
            "assigned",
            "request:needinfo",
            "request:approval-mozilla-beta",
            "request:review",
            "request:*",
        ]
    );
}

#[tokio::test]
async fn rejected_keys_need_reconnecting() {
    let bugzilla = MockServer::start().await;
    let app = start().await;
    Mock::given(path("/rest/whoami"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": 42, "name": ME })))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&bugzilla)
        .await;
    Mock::given(path("/rest/whoami"))
        .respond_with(invalid_key())
        .mount(&bugzilla)
        .await;

    let (_, account) = add_account(&app, &bugzilla, KEY).await;
    app.request(
        Method::POST,
        "/api/sources",
        Some(json!({
            "type": "bugzilla",
            "name": "Bugs",
            "contexts": ["default"],
            "config": { "account": account["id"], "show": "assigned" },
        })),
    )
    .await;
    app.refresh("default").await;
    assert_eq!(
        last_error(&app).await,
        format!("Bugzilla account {ME} needs reconnecting in settings")
    );
    assert_eq!(app.get("/api/accounts").await[0]["revoked"], true);
}

#[tokio::test]
async fn sources_need_something_to_show() {
    let bugzilla = MockServer::start().await;
    let app = start().await;
    mock_whoami(&bugzilla).await;
    let (_, account) = add_account(&app, &bugzilla, KEY).await;
    let (status, body) = app
        .request(
            Method::POST,
            "/api/sources",
            Some(json!({
                "type": "bugzilla",
                "name": "Bugs",
                "contexts": ["default"],
                "config": { "account": account["id"], "show": "request:" },
            })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "Choose what to show");
}

#[tokio::test]
async fn requests_are_due_after_they_were_made() {
    let bugzilla = MockServer::start().await;
    let app = start().await;
    let mut asked = flag(
        "needinfo",
        "?",
        ME,
        "alice@example.com",
        "2026-02-06T09:00:00Z",
    );
    // Requested on Friday, changed later.
    asked["modification_date"] = json!("2026-02-10T09:00:00Z");
    Mock::given(path("/rest/bug"))
        .and(query_param("f1", "requestees.login_name"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "bugs": [bug(1, json!([asked]))] })),
        )
        .mount(&bugzilla)
        .await;
    mock_whoami(&bugzilla).await;
    let (_, account) = add_account(&app, &bugzilla, KEY).await;
    let (status, body) = app
        .request(
            Method::POST,
            "/api/sources",
            Some(json!({
                "type": "bugzilla",
                "name": "Bugs",
                "contexts": ["default"],
                "config": { "account": account["id"], "show": "request:needinfo" },
                "default_due": { "days": 2, "business_days": true },
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    app.refresh("default").await;
    let items = app.get("/api/contexts/default/items").await;
    // Friday + 2 business days is Tuesday.
    assert_eq!(items[0]["due"], "2026-02-10T09:00:00Z");
}
