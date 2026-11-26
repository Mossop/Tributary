//! Items, refreshing and events.

mod common;

use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

use common::{TestApp, item, item_with_action, start, start_with, time, wait_for};

/// Contexts `work` and `home`; source `a` in work and `b` in both.
async fn setup() -> TestApp {
    let app = start().await;
    app.add_context("work").await;
    app.add_context("home").await;
    app.add_source("a", &["work"]).await;
    app.add_source("b", &["work", "home"]).await;
    app
}

fn item_path(app: &TestApp, key: &str, item: &str, action: &str) -> String {
    format!("/api/items/{}/{item}/{action}", app.id(key))
}

#[tokio::test]
async fn lists_sources_with_status() {
    let app = setup().await;
    app.source("a").set_error("boom");
    app.refresh("work").await;

    let sources = app.get("/api/sources").await;
    assert_eq!(sources[0]["id"], app.id("a"));
    assert_eq!(sources[0]["type"], "test");
    assert_eq!(sources[0]["name"], "Source a");
    assert_eq!(sources[0]["icon"], "symbol:science");
    assert_eq!(sources[0]["contexts"], json!(["work"]));
    assert_eq!(sources[0]["status"]["last_error"], "boom");

    assert_eq!(sources[1]["id"], app.id("b"));
    assert!(sources[1]["status"]["last_error"].is_null());
    assert!(sources[1]["status"]["last_success"].is_string());
}

#[tokio::test]
async fn items_are_filtered_by_context_and_sorted() {
    let app = setup().await;
    app.source("a")
        .set_items(vec![item("1", 5, None), item("2", 1, Some(20))]);
    app.source("b")
        .set_items(vec![item("3", 2, None), item("4", 1, Some(10))]);
    app.refresh("work").await;

    assert_eq!(
        app.ids("/api/contexts/work/items").await,
        // Due first, soonest first, then most recently modified first.
        ["b/4", "a/2", "a/1", "b/3"]
    );
    assert_eq!(app.ids("/api/contexts/home/items").await, ["b/4", "b/3"]);
    assert!(app.ids("/api/contexts/default/items").await.is_empty());

    let items = app.get("/api/contexts/home/items").await;
    assert_eq!(
        items[0],
        json!({
            "source_id": app.id("b"),
            "id": "4",
            "kind": "task",
            "title": "Item 4",
            "url": "https://example.com/4",
            "icon": null,
            "description": null,
            "modified": "2026-01-01T00:00:00Z",
            "due": "2026-01-10T00:00:00Z",
            "due_override": null,
            "snoozed_until": null,
            "snoozed_forever": false,
            "actions": [],
            "references": [],
        })
    );
}

#[tokio::test]
async fn unknown_context_and_source_are_not_found() {
    let app = setup().await;
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
    let app = setup().await;
    app.refresh("home").await;
    assert!(app.ids("/api/contexts/home/items").await.is_empty());

    app.source("b").set_items(vec![item("1", 1, None)]);
    let before = app.source("b").fetches();
    let (status, _) = app
        .request(
            Method::POST,
            &format!("/api/sources/{}/refresh", app.id("b")),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(app.source("b").fetches() > before);
    assert_eq!(app.ids("/api/contexts/home/items").await, ["b/1"]);
}

#[tokio::test]
async fn failed_fetch_keeps_previous_items() {
    let app = setup().await;
    app.source("b").set_items(vec![item("1", 1, None)]);
    app.refresh("home").await;
    app.source("b").set_error("offline");
    app.refresh("home").await;
    assert_eq!(app.ids("/api/contexts/home/items").await, ["b/1"]);
}

#[tokio::test]
async fn snooze_and_unsnooze() {
    let app = setup().await;
    // Ids containing characters that must be encoded in a path segment.
    app.source("b").set_items(vec![
        item("owner/repo#1", 1, None),
        item("owner/repo#2", 2, None),
    ]);
    app.refresh("home").await;

    let path = item_path(&app, "b", "owner%2Frepo%231", "snooze");
    let (status, _) = app
        .request(
            Method::PUT,
            &path,
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

    let (status, _) = app.request(Method::DELETE, &path, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(app.ids("/api/contexts/home/items").await.len(), 2);
    assert!(app.ids("/api/contexts/home/snoozed").await.is_empty());

    // Snooze forever.
    let (status, _) = app
        .request(Method::PUT, &path, Some(json!({ "until": null })))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let snoozed = app.get("/api/contexts/home/snoozed").await;
    assert_eq!(snoozed[0]["snoozed_forever"], true);
    assert!(snoozed[0]["snoozed_until"].is_null());
}

#[tokio::test]
async fn snooze_in_the_past_has_no_effect() {
    let app = setup().await;
    app.source("b").set_items(vec![item("1", 1, None)]);
    app.refresh("home").await;
    app.request(
        Method::PUT,
        &item_path(&app, "b", "1", "snooze"),
        Some(json!({ "until": time(1) })),
    )
    .await;
    assert_eq!(app.ids("/api/contexts/home/items").await, ["b/1"]);
}

#[tokio::test]
async fn state_changes_require_known_item() {
    let app = setup().await;
    app.refresh("home").await;
    let (status, body) = app
        .request(
            Method::PUT,
            &item_path(&app, "b", "missing", "snooze"),
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
    let app = setup().await;
    app.source("b")
        .set_items(vec![item("1", 1, Some(10)), item("2", 2, None)]);
    app.refresh("home").await;

    let path = item_path(&app, "b", "2", "due");
    let (status, _) = app
        .request(
            Method::PUT,
            &path,
            Some(json!({ "due": "2026-01-05T00:00:00Z" })),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let items = app.get("/api/contexts/home/items").await;
    assert_eq!(items[0]["id"], "2");
    assert_eq!(items[0]["due_override"], "2026-01-05T00:00:00Z");

    let (status, _) = app.request(Method::DELETE, &path, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(app.ids("/api/contexts/home/items").await, ["b/1", "b/2"]);
}

#[tokio::test]
async fn activity_after_idle_refreshes_all_sources() {
    let app = setup().await;
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
async fn events_stream_ends_on_shutdown() {
    let app = start().await;
    let response = app
        .router
        .clone()
        .oneshot(Request::get("/api/events").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let body = response.into_body();

    app.workspace.shutdown();
    tokio::time::timeout(Duration::from_secs(5), body.collect())
        .await
        .expect("the stream should end")
        .unwrap();
}

#[tokio::test]
async fn events_stream_reports_changes() {
    let app = setup().await;
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

    let b = app.id("b");
    app.request(Method::POST, &format!("/api/sources/{b}/refresh"), None)
        .await;
    app.request(Method::DELETE, &item_path(&app, "b", "1", "due"), None)
        .await;
    app.add_context("extra").await;

    let mut received = String::new();
    while !received.contains("settings-changed") {
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
        received.contains(&format!(
            r#"data: {{"type":"source-updated","source_id":"{b}"}}"#
        )),
        "{received}"
    );
    assert!(
        received.contains(&format!(
            r#"data: {{"type":"item-state-changed","source_id":"{b}","item_id":"1"}}"#
        )),
        "{received}"
    );
    assert!(
        received.contains(r#"data: {"type":"settings-changed"}"#),
        "{received}"
    );
}

#[tokio::test]
async fn serves_frontend_with_fallback() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("index.html"), "<p>index</p>").unwrap();
    std::fs::write(dir.path().join("app.js"), "// js").unwrap();
    let config = format!(
        "[server]\nstatic_dir = {:?}\n",
        dir.path().to_str().unwrap()
    );
    let app = start_with(&config, tributary::db::connect_in_memory().await.unwrap()).await;

    let (status, body) = app.request(Method::GET, "/app.js", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "// js");

    let (status, body) = app.request(Method::GET, "/c/work", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "<p>index</p>");
}

#[tokio::test]
async fn serves_no_frontend_without_a_static_dir() {
    let config = "[server]\nstatic_dir = \"\"\n";
    let app = start_with(config, tributary::db::connect_in_memory().await.unwrap()).await;
    let (status, _) = app.request(Method::GET, "/c/work", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = app.request(Method::GET, "/api/contexts", None).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn performs_quick_actions_and_refreshes() {
    let app = setup().await;
    app.source("a")
        .set_items(vec![item_with_action("1", 2), item("2", 1, None)]);
    app.refresh("work").await;

    let items = app.get("/api/contexts/work/items").await;
    assert_eq!(
        items[0]["actions"],
        json!([{ "id": "done", "label": "Done", "icon": "symbol:check" }])
    );

    let fetches = app.source("a").fetches();
    let (status, body) = app
        .request(
            Method::POST,
            &item_path(&app, "a", "1", "actions/done"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert_eq!(
        app.source("a").performed(),
        [("1".to_string(), "done".to_string())]
    );
    // The source was refreshed before responding, so the item is gone.
    assert!(app.source("a").fetches() > fetches);
    assert_eq!(app.ids("/api/contexts/work/items").await, ["a/2"]);
}

#[tokio::test]
async fn rejects_unknown_quick_actions() {
    let app = setup().await;
    app.source("a")
        .set_items(vec![item_with_action("1", 1), item("2", 2, None)]);
    app.refresh("work").await;

    for (key, item, action, error) in [
        ("a", "1", "other", "unknown action `other`"),
        ("a", "2", "done", "unknown action `done`"),
        ("a", "missing", "done", "unknown item `missing`"),
    ] {
        let (status, body) = app
            .request(
                Method::POST,
                &item_path(&app, key, item, &format!("actions/{action}")),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"], error);
    }
    let (status, _) = app
        .request(Method::POST, "/api/items/nope/1/actions/done", None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(app.source("a").performed().is_empty());
}

#[tokio::test]
async fn reports_failed_quick_actions() {
    let app = setup().await;
    app.source("a").set_items(vec![item_with_action("1", 1)]);
    app.refresh("work").await;
    app.source("a").set_perform_error("no permission");

    let (status, body) = app
        .request(
            Method::POST,
            &item_path(&app, "a", "1", "actions/done"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(body["error"], "no permission");
    assert_eq!(app.ids("/api/contexts/work/items").await, ["a/1"]);
}

#[tokio::test]
async fn link_actions_are_not_performed() {
    let app = setup().await;
    let mut linked = item_with_action("1", 1);
    linked.actions[0].url = Some("https://example.com/land".to_string());
    app.source("a").set_items(vec![linked]);
    app.refresh("work").await;

    let items = app.get("/api/contexts/work/items").await;
    assert_eq!(items[0]["actions"][0]["url"], "https://example.com/land");

    let (status, body) = app
        .request(
            Method::POST,
            &item_path(&app, "a", "1", "actions/done"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "unknown action `done`");
    assert!(app.source("a").performed().is_empty());
}

#[tokio::test]
async fn items_carry_their_references() {
    let app = setup().await;
    let mut referencing = item("1", 1, None);
    referencing.references = vec!["bugzilla:1".to_string(), "phab:D2".to_string()];
    app.source("a").set_items(vec![referencing]);
    app.refresh("work").await;
    let items = app.get("/api/contexts/work/items").await;
    assert_eq!(items[0]["references"], json!(["bugzilla:1", "phab:D2"]));
}
