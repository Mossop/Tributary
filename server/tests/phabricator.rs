//! Phabricator accounts and the Phabricator source, against a mocked Conduit.

mod common;

use axum::http::{Method, StatusCode};
use reqwest::Url;
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Match, Mock, MockServer, Request, ResponseTemplate};

use common::{TestApp, start};

const ME: &str = "PHID-USER-me";
const ALICE: &str = "PHID-USER-alice";
const COLLEAGUE: &str = "PHID-USER-colleague";
const GROUP: &str = "PHID-PROJ-group";

/// The `params` JSON of a Conduit call.
fn params(request: &Request) -> Value {
    let body = String::from_utf8_lossy(&request.body);
    let url = Url::parse(&format!("http://local/?{body}")).unwrap();
    let params = url
        .query_pairs()
        .find(|(k, _)| k == "params")
        .map(|(_, v)| v.into_owned())
        .unwrap_or_default();
    serde_json::from_str(&params).unwrap_or_default()
}

/// Matches Conduit calls whose params satisfy a check.
struct Params(Box<dyn Fn(&Value) -> bool + Send + Sync>);

impl Match for Params {
    fn matches(&self, request: &Request) -> bool {
        (self.0)(&params(request))
    }
}

fn with_token(token: &'static str) -> Params {
    Params(Box::new(move |p| p["__conduit__"]["token"] == token))
}

fn ok(result: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "result": result,
        "error_code": null,
        "error_info": null,
    }))
}

fn rejected() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "result": null,
        "error_code": "ERR-INVALID-AUTH",
        "error_info": "API token \"api-...\" has the wrong length.",
    }))
}

/// One page of search results.
fn page(data: Value, after: Option<&str>) -> ResponseTemplate {
    ok(json!({ "data": data, "cursor": { "limit": 100, "after": after, "before": null } }))
}

async fn mock_whoami(conduit: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/api/user.whoami"))
        .and(with_token("api-token"))
        .respond_with(ok(
            json!({ "phid": ME, "userName": "dave", "realName": "Dave" }),
        ))
        .mount(conduit)
        .await;
}

async fn add_account(app: &TestApp, conduit: &MockServer, token: &str) -> (StatusCode, Value) {
    app.request(
        Method::POST,
        "/api/accounts/phabricator",
        Some(json!({ "token": token, "api_url": format!("{}/api", conduit.uri()) })),
    )
    .await
}

/// Adds the account and a source showing `list`.
async fn add_source(app: &TestApp, conduit: &MockServer, list: &str) {
    add_source_with(app, conduit, list, Value::Null).await;
}

/// Adds the account and a source showing `list` with `default_due`.
async fn add_source_with(app: &TestApp, conduit: &MockServer, list: &str, default_due: Value) {
    mock_whoami(conduit).await;
    let (status, account) = add_account(app, conduit, "api-token").await;
    assert_eq!(status, StatusCode::CREATED, "{account}");
    let (status, body) = app
        .request(
            Method::POST,
            "/api/sources",
            Some(json!({
                "type": "phabricator",
                "name": "Phabricator",
                "contexts": ["default"],
                "config": { "account": account["id"], "list": list },
                "default_due": default_due,
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

/// How many times revisions' histories were fetched.
async fn history_calls(conduit: &MockServer) -> usize {
    conduit
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == "/api/transaction.search")
        .count()
}

/// Mocks a revision's history.
async fn mock_history(conduit: &MockServer, revision: &'static str, history: Value) {
    Mock::given(path("/api/transaction.search"))
        .and(Params(Box::new(move |p| p["objectIdentifier"] == revision)))
        .respond_with(page(history, None))
        .mount(conduit)
        .await;
}

fn transaction(kind: &str, at: i64, fields: Value) -> Value {
    json!({ "id": at, "type": kind, "dateCreated": at, "fields": fields })
}

fn revision(id: u64, author: &str, status: (&str, &str), reviewers: &[(&str, &str)]) -> Value {
    json!({
        "id": id,
        "phid": format!("PHID-DREV-{id}"),
        "fields": {
            "title": format!("Bug {id} - Change {id}"),
            "uri": format!("https://phab.test/D{id}"),
            "authorPHID": author,
            "status": { "value": status.0, "name": status.1, "closed": false },
            "dateModified": 1_700_000_000 + id,
            "isDraft": false,
        },
        "attachments": {
            "reviewers": {
                "reviewers": reviewers
                    .iter()
                    .map(|(phid, status)| json!({
                        "reviewerPHID": phid,
                        "status": status,
                        "isBlocking": *status == "blocking",
                        "actorPHID": null,
                    }))
                    .collect::<Vec<_>>(),
            },
        },
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
async fn adds_accounts_from_tokens() {
    let conduit = MockServer::start().await;
    let app = start().await;
    mock_whoami(&conduit).await;

    let (status, body) = add_account(&app, &conduit, " api-token ").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = body["id"].clone();
    assert_eq!(
        app.get("/api/accounts").await,
        json!([{
            "id": id,
            "provider": "phabricator",
            "name": "dave",
            "api_url": format!("{}/api", conduit.uri()),
            "missing_scopes": false,
            "revoked": false,
        }])
    );

    // The same user again updates the account.
    let (_, again) = add_account(&app, &conduit, "api-token").await;
    assert_eq!(again["id"], id);
    assert_eq!(app.get("/api/accounts").await.as_array().unwrap().len(), 1);

    let providers = app.get("/api/account-providers").await;
    let phabricator = providers
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "phabricator")
        .unwrap();
    assert_eq!(phabricator["connect"], "token");
    assert_eq!(
        phabricator["api_url_placeholder"],
        "https://phabricator.services.mozilla.com/api"
    );
}

#[tokio::test]
async fn rejects_bad_tokens() {
    let conduit = MockServer::start().await;
    let app = start().await;
    Mock::given(path("/api/user.whoami"))
        .respond_with(rejected())
        .mount(&conduit)
        .await;
    let (status, body) = add_account(&app, &conduit, "api-wrong").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "Phabricator didn't accept the token");
    assert_eq!(app.get("/api/accounts").await, json!([]));
}

#[tokio::test]
async fn offers_the_three_lists() {
    let app = start().await;
    let (status, options) = app
        .request(
            Method::POST,
            "/api/source-types/phabricator/fields/list/options",
            Some(json!({ "config": {} })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        options,
        json!([
            { "value": "review", "label": "Patches to review" },
            { "value": "work", "label": "Patches to work on" },
            { "value": "land", "label": "Patches ready to land" },
        ])
    );
}

#[tokio::test]
async fn lists_patches_waiting_for_my_review() {
    let conduit = MockServer::start().await;
    let app = start().await;
    Mock::given(path("/api/project.search"))
        .and(Params(Box::new(|p| {
            p["constraints"]["members"] == json!([ME]) && p["attachments"]["members"] == true
        })))
        .respond_with(page(
            json!([{
                "id": 1,
                "phid": GROUP,
                "fields": { "name": "Group reviewers", "slug": "group-reviewers" },
                "attachments": { "members": { "members": [{ "phid": ME }, { "phid": COLLEAGUE }] } },
            }]),
            None,
        ))
        .mount(&conduit)
        .await;
    let needs_review = ("needs-review", "Needs Review");
    Mock::given(path("/api/differential.revision.search"))
        .and(Params(Box::new(|p| {
            let reviewers = p["constraints"]["reviewerPHIDs"].as_array();
            p["constraints"]["statuses"] == json!(["needs-review"])
                && reviewers.is_some_and(|r| r.contains(&json!(ME)) && r.contains(&json!(GROUP)))
                && p["attachments"]["reviewers"] == true
        })))
        .respond_with(page(
            json!([
                // Asked of me directly.
                revision(1, ALICE, needs_review, &[(ME, "added")]),
                // I already accepted.
                revision(2, ALICE, needs_review, &[(ME, "accepted")]),
                // Asked of my group.
                revision(3, ALICE, needs_review, &[(GROUP, "added")]),
                // My group, but a colleague from it is reviewing.
                revision(
                    4,
                    ALICE,
                    needs_review,
                    &[(GROUP, "added"), (COLLEAGUE, "added")]
                ),
                // My group, and the colleague resigned.
                revision(
                    5,
                    ALICE,
                    needs_review,
                    &[(GROUP, "blocking"), (COLLEAGUE, "resigned")]
                ),
                // My own.
                revision(6, ME, needs_review, &[(GROUP, "added")]),
                // I reviewed an older diff.
                revision(7, ALICE, needs_review, &[(ME, "accepted-older")]),
            ]),
            None,
        ))
        .mount(&conduit)
        .await;
    Mock::given(path("/api/user.search"))
        .and(Params(Box::new(|p| {
            p["constraints"]["phids"] == json!([ALICE])
        })))
        .respond_with(page(
            json!([{ "phid": ALICE, "fields": { "username": "alice" } }]),
            None,
        ))
        .mount(&conduit)
        .await;

    add_source(&app, &conduit, "review").await;
    app.refresh("default").await;
    assert_eq!(last_error(&app).await, Value::Null);
    assert_eq!(
        items(&app).await,
        [
            ("D1".to_string(), json!("D1 · by alice")),
            (
                "D3".to_string(),
                json!("D3 · by alice · for #group-reviewers")
            ),
            (
                "D5".to_string(),
                json!("D5 · by alice · for #group-reviewers")
            ),
            ("D7".to_string(), json!("D7 · by alice")),
        ]
    );
    // Without a default due date, histories aren't fetched.
    assert_eq!(history_calls(&conduit).await, 0);
    let item = &app.get("/api/contexts/default/items").await[0];
    assert_eq!(item["kind"], "patch");
    assert_eq!(item["title"], "Bug 1 - Change 1");
    assert_eq!(item["url"], "https://phab.test/D1");
    assert_eq!(item["modified"], "2023-11-14T22:13:21Z");
    assert_eq!(item["actions"], json!([]));
    // Its own id, and the bug from its title.
    assert_eq!(item["references"], json!(["phab:D1", "bugzilla:1"]));
}

#[tokio::test]
async fn lists_my_patches_to_work_on_across_pages() {
    let conduit = MockServer::start().await;
    let app = start().await;
    let constraints = |p: &Value| {
        p["constraints"]["authorPHIDs"] == json!([ME])
            && p["constraints"]["statuses"] == json!(["draft", "needs-revision", "changes-planned"])
    };
    Mock::given(path("/api/differential.revision.search"))
        .and(Params(Box::new(move |p| {
            constraints(p) && p["after"] == "next"
        })))
        .respond_with(page(
            json!([revision(9, ME, ("draft", "Draft"), &[])]),
            None,
        ))
        .with_priority(1)
        .mount(&conduit)
        .await;
    Mock::given(path("/api/differential.revision.search"))
        .and(Params(Box::new(constraints)))
        .respond_with(page(
            json!([revision(8, ME, ("needs-revision", "Needs Revision"), &[])]),
            Some("next"),
        ))
        .mount(&conduit)
        .await;

    add_source(&app, &conduit, "work").await;
    app.refresh("default").await;
    assert_eq!(last_error(&app).await, Value::Null);
    assert_eq!(
        items(&app).await,
        [
            ("D8".to_string(), json!("D8 · Needs Revision")),
            ("D9".to_string(), json!("D9 · Draft")),
        ]
    );
}

#[tokio::test]
async fn lists_my_patches_ready_to_land() {
    let conduit = MockServer::start().await;
    let app = start().await;
    Mock::given(path("/api/differential.revision.search"))
        .and(Params(Box::new(|p| {
            p["constraints"]["authorPHIDs"] == json!([ME])
                && p["constraints"]["statuses"] == json!(["accepted"])
        })))
        .respond_with(page(
            json!([revision(10, ME, ("accepted", "Accepted"), &[])]),
            None,
        ))
        .mount(&conduit)
        .await;

    add_source(&app, &conduit, "land").await;
    app.refresh("default").await;
    assert_eq!(
        items(&app).await,
        [("D10".to_string(), json!("D10 · Accepted"))]
    );
    // Lando only knows about Mozilla's Phabricator, not this mock.
    assert_eq!(
        app.get("/api/contexts/default/items").await[0]["actions"],
        json!([])
    );
}

#[tokio::test]
async fn rejected_tokens_need_reconnecting() {
    let conduit = MockServer::start().await;
    let app = start().await;
    // Accepted when the account is added, then revoked.
    Mock::given(path("/api/user.whoami"))
        .respond_with(ok(json!({ "phid": ME, "userName": "dave" })))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&conduit)
        .await;
    Mock::given(path("/api/user.whoami"))
        .respond_with(rejected())
        .mount(&conduit)
        .await;

    let (_, account) = add_account(&app, &conduit, "api-token").await;
    app.request(
        Method::POST,
        "/api/sources",
        Some(json!({
            "type": "phabricator",
            "name": "Phabricator",
            "contexts": ["default"],
            "config": { "account": account["id"], "list": "work" },
        })),
    )
    .await;
    app.refresh("default").await;
    assert_eq!(
        last_error(&app).await,
        "Phabricator account dave needs reconnecting in settings"
    );
    assert_eq!(app.get("/api/accounts").await[0]["revoked"], true);
}

#[tokio::test]
async fn sources_need_a_list() {
    let conduit = MockServer::start().await;
    let app = start().await;
    mock_whoami(&conduit).await;
    let (_, account) = add_account(&app, &conduit, "api-token").await;
    let (status, body) = app
        .request(
            Method::POST,
            "/api/sources",
            Some(json!({
                "type": "phabricator",
                "name": "Phabricator",
                "contexts": ["default"],
                "config": { "account": account["id"], "list": "bogus" },
            })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "Choose what to show");
}

/// 2026-02-02 09:00 UTC, a Monday.
const MONDAY: i64 = 1_770_022_800;
const HOUR: i64 = 3600;

#[tokio::test]
async fn reviews_are_due_after_they_were_requested() {
    let conduit = MockServer::start().await;
    let app = start().await;
    Mock::given(path("/api/project.search"))
        .respond_with(page(
            json!([{
                "id": 1,
                "phid": GROUP,
                "fields": { "name": "Group reviewers", "slug": "group-reviewers" },
                "attachments": { "members": { "members": [{ "phid": ME }] } },
            }]),
            None,
        ))
        .mount(&conduit)
        .await;
    let needs_review = ("needs-review", "Needs Review");
    Mock::given(path("/api/differential.revision.search"))
        .respond_with(page(
            json!([
                revision(1, ALICE, needs_review, &[(ME, "added")]),
                revision(2, ALICE, needs_review, &[(GROUP, "added")]),
            ]),
            None,
        ))
        .mount(&conduit)
        .await;
    Mock::given(path("/api/user.search"))
        .respond_with(page(
            json!([{ "phid": ALICE, "fields": { "username": "alice" } }]),
            None,
        ))
        .mount(&conduit)
        .await;
    let added = |phid: &str, at: i64| {
        transaction(
            "reviewers",
            at,
            json!({ "operations": [{ "operation": "add", "phid": phid, "newStatus": "added" }] }),
        )
    };
    mock_history(&conduit, "D1", json!([added(ME, MONDAY)])).await;
    mock_history(&conduit, "D2", json!([added(GROUP, MONDAY + 2 * HOUR)])).await;

    add_source_with(
        &app,
        &conduit,
        "review",
        json!({ "days": 3, "business_days": false }),
    )
    .await;
    app.refresh("default").await;
    assert_eq!(last_error(&app).await, Value::Null);
    let items = app.get("/api/contexts/default/items").await;
    let dues: Vec<(&str, &str)> = items
        .as_array()
        .unwrap()
        .iter()
        .map(|i| (i["id"].as_str().unwrap(), i["due"].as_str().unwrap()))
        .collect();
    assert_eq!(
        dues,
        [
            ("D1", "2026-02-05T09:00:00Z"),
            ("D2", "2026-02-05T11:00:00Z")
        ]
    );
}

#[tokio::test]
async fn landing_is_due_after_acceptance() {
    let conduit = MockServer::start().await;
    let app = start().await;
    Mock::given(path("/api/differential.revision.search"))
        .respond_with(page(
            json!([revision(10, ME, ("accepted", "Accepted"), &[])]),
            None,
        ))
        .mount(&conduit)
        .await;
    mock_history(
        &conduit,
        "D10",
        json!([
            transaction("accept", MONDAY - HOUR, json!({})),
            transaction(
                "status",
                MONDAY,
                json!({ "old": "needs-review", "new": "accepted" })
            ),
        ]),
    )
    .await;

    add_source_with(
        &app,
        &conduit,
        "land",
        json!({ "days": 1, "business_days": true }),
    )
    .await;
    app.refresh("default").await;
    let items = app.get("/api/contexts/default/items").await;
    assert_eq!(items[0]["due"], "2026-02-03T09:00:00Z");
}
