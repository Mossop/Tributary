mod common;

use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

use common::{item, start, time, wait_for};

const CONFIG: &str = r#"
    [[contexts]]
    id = "work"
    name = "Work"
    icon = "work"

    [[contexts]]
    id = "home"
    name = "Home"

    [[sources]]
    id = "a"
    type = "stub"
    name = "Source A"
    contexts = ["work"]

    [[sources]]
    id = "b"
    type = "stub"
    name = "Source B"
    contexts = ["work", "home"]
"#;

#[tokio::test]
async fn lists_contexts() {
    let app = start(CONFIG).await;
    assert_eq!(
        app.get("/api/contexts").await,
        json!([
            { "id": "work", "name": "Work", "icon": "work" },
            { "id": "home", "name": "Home", "icon": null },
        ])
    );
}

#[tokio::test]
async fn lists_sources_with_status() {
    let app = start(CONFIG).await;
    app.source("a").set_error("boom");
    app.refresh("work").await;

    let sources = app.get("/api/sources").await;
    assert_eq!(sources[0]["id"], "a");
    assert_eq!(sources[0]["name"], "Source A");
    assert_eq!(sources[0]["icon"], "symbol:science");
    assert_eq!(sources[0]["contexts"], json!(["work"]));
    assert_eq!(sources[0]["status"]["last_error"], "boom");
    assert!(sources[0]["status"]["last_success"].is_null());

    assert_eq!(sources[1]["id"], "b");
    assert!(sources[1]["status"]["last_error"].is_null());
    assert!(sources[1]["status"]["last_success"].is_string());
}

#[tokio::test]
async fn items_are_filtered_by_context_and_sorted() {
    let app = start(CONFIG).await;
    app.source("a")
        .set_items(vec![item("1", 5, None), item("2", 1, Some(20))]);
    app.source("b")
        .set_items(vec![item("3", 2, None), item("4", 1, Some(10))]);
    app.refresh("work").await;

    assert_eq!(
        app.ids("/api/contexts/work/items").await,
        ["b/4", "a/2", "b/3", "a/1"]
    );
    assert_eq!(app.ids("/api/contexts/home/items").await, ["b/4", "b/3"]);

    let items = app.get("/api/contexts/home/items").await;
    assert_eq!(
        items[0],
        json!({
            "source_id": "b",
            "id": "4",
            "title": "Item 4",
            "url": "https://example.com/4",
            "icon": null,
            "description": null,
            "modified": "2026-01-01T00:00:00Z",
            "due": "2026-01-10T00:00:00Z",
            "due_override": null,
            "snoozed_until": null,
            "snoozed_forever": false,
        })
    );
}

#[tokio::test]
async fn unknown_context_and_source_are_not_found() {
    let app = start(CONFIG).await;
    let (status, body) = app
        .request(Method::GET, "/api/contexts/nope/items", None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "unknown context `nope`");

    let (status, _) = app
        .request(Method::POST, "/api/sources/nope/refresh", None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, body) = app.request(Method::GET, "/api/nothing", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "not found");
}

#[tokio::test]
async fn refresh_fetches_and_updates_items() {
    let app = start(CONFIG).await;
    app.refresh("home").await;
    assert!(app.ids("/api/contexts/home/items").await.is_empty());

    app.source("b").set_items(vec![item("1", 1, None)]);
    let before = app.source("b").fetches();
    let (status, _) = app
        .request(Method::POST, "/api/sources/b/refresh", None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(app.source("b").fetches() > before);
    assert_eq!(app.ids("/api/contexts/home/items").await, ["b/1"]);
}

#[tokio::test]
async fn failed_fetch_keeps_previous_items() {
    let app = start(CONFIG).await;
    app.source("b").set_items(vec![item("1", 1, None)]);
    app.refresh("home").await;
    app.source("b").set_error("offline");
    app.refresh("home").await;
    assert_eq!(app.ids("/api/contexts/home/items").await, ["b/1"]);
}

#[tokio::test]
async fn snooze_and_unsnooze() {
    let app = start(CONFIG).await;
    // Ids containing characters that must be encoded in a path segment.
    app.source("b").set_items(vec![
        item("owner/repo#1", 1, None),
        item("owner/repo#2", 2, None),
    ]);
    app.refresh("home").await;

    let path = "/api/items/b/owner%2Frepo%231/snooze";
    let (status, _) = app
        .request(
            Method::PUT,
            path,
            Some(json!({ "until": "2099-01-01T00:00:00Z" })),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        app.ids("/api/contexts/home/items").await,
        ["b/owner/repo#2"]
    );

    let snoozed = app.get("/api/contexts/home/snoozed").await;
    assert_eq!(snoozed.as_array().unwrap().len(), 1);
    assert_eq!(snoozed[0]["id"], "owner/repo#1");
    assert_eq!(snoozed[0]["snoozed_until"], "2099-01-01T00:00:00Z");

    let (status, _) = app.request(Method::DELETE, path, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(app.ids("/api/contexts/home/items").await.len(), 2);
    assert!(app.ids("/api/contexts/home/snoozed").await.is_empty());

    // Snooze forever.
    let (status, _) = app
        .request(Method::PUT, path, Some(json!({ "until": null })))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let snoozed = app.get("/api/contexts/home/snoozed").await;
    assert_eq!(snoozed[0]["snoozed_forever"], true);
    assert!(snoozed[0]["snoozed_until"].is_null());
}

#[tokio::test]
async fn snooze_in_the_past_has_no_effect() {
    let app = start(CONFIG).await;
    app.source("b").set_items(vec![item("1", 1, None)]);
    app.refresh("home").await;
    app.request(
        Method::PUT,
        "/api/items/b/1/snooze",
        Some(json!({ "until": time(1) })),
    )
    .await;
    assert_eq!(app.ids("/api/contexts/home/items").await, ["b/1"]);
}

#[tokio::test]
async fn state_changes_require_known_item() {
    let app = start(CONFIG).await;
    app.refresh("home").await;
    let (status, body) = app
        .request(
            Method::PUT,
            "/api/items/b/missing/snooze",
            Some(json!({ "until": null })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "unknown item `missing`");

    let (status, _) = app
        .request(Method::DELETE, "/api/items/zzz/1/due", None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn due_override() {
    let app = start(CONFIG).await;
    app.source("b")
        .set_items(vec![item("1", 1, Some(10)), item("2", 2, None)]);
    app.refresh("home").await;

    let (status, _) = app
        .request(
            Method::PUT,
            "/api/items/b/2/due",
            Some(json!({ "due": "2026-01-05T00:00:00Z" })),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let items = app.get("/api/contexts/home/items").await;
    assert_eq!(items[0]["id"], "2");
    assert_eq!(items[0]["due_override"], "2026-01-05T00:00:00Z");

    let (status, _) = app
        .request(Method::DELETE, "/api/items/b/2/due", None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(app.ids("/api/contexts/home/items").await, ["b/1", "b/2"]);
}

#[tokio::test]
async fn activity_after_idle_refreshes_all_sources() {
    let app = start(CONFIG).await;
    wait_for(|| app.source("a").fetches() == 1 && app.source("b").fetches() == 1).await;

    let (status, _) = app.request(Method::POST, "/api/activity", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    wait_for(|| app.source("a").fetches() == 2 && app.source("b").fetches() == 2).await;

    // Already active, so no further refresh.
    app.request(Method::POST, "/api/activity", None).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(app.source("a").fetches(), 2);
}

#[tokio::test]
async fn events_stream_reports_changes() {
    let app = start(CONFIG).await;
    app.source("b").set_items(vec![item("1", 1, None)]);
    app.refresh("home").await;

    let response = app
        .router
        .clone()
        .oneshot(Request::get("/api/events").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    let mut body = response.into_body();

    app.request(Method::POST, "/api/sources/b/refresh", None)
        .await;
    app.request(Method::DELETE, "/api/items/b/1/due", None)
        .await;

    let mut received = String::new();
    while !received.contains("item-state-changed") {
        let frame = tokio::time::timeout(Duration::from_secs(5), body.frame())
            .await
            .expect("timed out waiting for event")
            .unwrap()
            .unwrap();
        if let Some(data) = frame.data_ref() {
            received.push_str(std::str::from_utf8(data).unwrap());
        }
    }
    assert!(
        received.contains(r#"data: {"type":"source-updated","source_id":"b"}"#),
        "{received}"
    );
    assert!(
        received.contains(r#"data: {"type":"item-state-changed","source_id":"b","item_id":"1"}"#),
        "{received}"
    );
}

#[tokio::test]
async fn serves_frontend_with_fallback() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("index.html"), "<p>index</p>").unwrap();
    std::fs::write(dir.path().join("app.js"), "// js").unwrap();
    let config = format!(
        "[server]\nstatic_dir = {:?}\n{CONFIG}",
        dir.path().to_str().unwrap()
    );
    let app = start(&config).await;

    let (status, body) = app.request(Method::GET, "/app.js", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "// js");

    let (status, body) = app.request(Method::GET, "/c/work", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "<p>index</p>");
}
