//! Managing contexts and sources at runtime.

mod common;

use std::time::Duration;

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};

use common::{item, start, start_polling, start_with, time, wait_for};
use tributary::db;
use tributary::settings::{self, SourceRecord};

#[tokio::test]
async fn creates_default_context() {
    let app = start().await;
    assert_eq!(
        app.get("/api/contexts").await,
        json!([{ "id": "default", "name": "Default", "icon": null, "hidden_kinds": [] }])
    );
}

#[tokio::test]
async fn creates_and_updates_contexts() {
    let app = start().await;
    let (status, body) = app
        .request(
            Method::POST,
            "/api/contexts",
            Some(json!({ "id": " work ", "name": " Work ", "icon": "work" })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        body,
        json!({ "id": "work", "name": "Work", "icon": "work", "hidden_kinds": [] })
    );

    let (status, _) = app
        .request(
            Method::PUT,
            "/api/contexts/default",
            Some(json!({ "id": "personal", "name": "Personal", "icon": "" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    assert_eq!(
        app.get("/api/contexts").await,
        json!([
            { "id": "personal", "name": "Personal", "icon": null, "hidden_kinds": [] },
            { "id": "work", "name": "Work", "icon": "work", "hidden_kinds": [] },
        ])
    );
}

#[tokio::test]
async fn contexts_remember_hidden_kinds() {
    let app = start().await;
    let mut events = app.workspace.events.subscribe();
    let (status, body) = app
        .request(
            Method::PUT,
            "/api/contexts/default/hidden-kinds",
            Some(json!(["task", "patch", "task"])),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["hidden_kinds"], json!(["patch", "task"]));
    assert_eq!(
        events.try_recv().unwrap(),
        tributary::events::Event::SettingsChanged
    );

    // Editing the context doesn't change them, even if the body has some.
    let (status, body) = app
        .request(
            Method::PUT,
            "/api/contexts/default",
            Some(json!({ "id": "default", "name": "Home", "hidden_kinds": [] })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["hidden_kinds"], json!(["patch", "task"]));
    assert_eq!(
        app.get("/api/contexts").await[0]["hidden_kinds"],
        json!(["patch", "task"])
    );

    let (status, _) = app
        .request(
            Method::PUT,
            "/api/contexts/default/hidden-kinds",
            Some(json!(["bogus"])),
        )
        .await;
    assert!(status.is_client_error());
    let (status, _) = app
        .request(
            Method::PUT,
            "/api/contexts/nope/hidden-kinds",
            Some(json!([])),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn validates_contexts() {
    let app = start().await;
    for (body, status, error) in [
        (
            json!({ "id": "Has Spaces", "name": "X" }),
            StatusCode::BAD_REQUEST,
            "URL name must be lowercase letters, digits and dashes",
        ),
        (
            json!({ "id": "-leading", "name": "X" }),
            StatusCode::BAD_REQUEST,
            "URL name must be lowercase letters, digits and dashes",
        ),
        (
            json!({ "id": "ok", "name": "  " }),
            StatusCode::BAD_REQUEST,
            "Name is required",
        ),
        (
            json!({ "id": "default", "name": "Again" }),
            StatusCode::CONFLICT,
            "a context with URL name `default` already exists",
        ),
    ] {
        let (actual, response) = app.request(Method::POST, "/api/contexts", Some(body)).await;
        assert_eq!(actual, status, "{response}");
        assert_eq!(response["error"], error);
    }

    app.add_context("work").await;
    let (status, _) = app
        .request(
            Method::PUT,
            "/api/contexts/work",
            Some(json!({ "id": "default", "name": "Work" })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, _) = app
        .request(
            Method::PUT,
            "/api/contexts/missing",
            Some(json!({ "id": "x", "name": "X" })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn renaming_context_keeps_its_sources() {
    let app = start().await;
    app.add_source("a", &["default"]).await;
    app.source("a").set_items(vec![item("1", 1, None)]);
    app.refresh("default").await;

    app.request(
        Method::PUT,
        "/api/contexts/default",
        Some(json!({ "id": "home", "name": "Home" })),
    )
    .await;
    assert_eq!(app.ids("/api/contexts/home/items").await, ["a/1"]);
    assert_eq!(
        app.get("/api/sources").await[0]["contexts"],
        json!(["home"])
    );
}

#[tokio::test]
async fn deleting_contexts() {
    let app = start().await;
    let (status, body) = app
        .request(Method::DELETE, "/api/contexts/default", None)
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "the last context can't be deleted");

    app.add_context("work").await;
    app.add_source("only", &["default"]).await;
    app.add_source("both", &["default", "work"]).await;

    let (status, body) = app
        .request(Method::DELETE, "/api/contexts/default", None)
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        body["error"],
        "these sources are only shown in this context, so move or delete them first: Source only"
    );

    app.request(
        Method::DELETE,
        &format!("/api/sources/{}", app.id("only")),
        None,
    )
    .await;
    let (status, _) = app
        .request(Method::DELETE, "/api/contexts/default", None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        app.get("/api/sources").await[0]["contexts"],
        json!(["work"])
    );

    let (status, _) = app
        .request(Method::DELETE, "/api/contexts/default", None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn lists_offered_source_types() {
    let app = start().await;
    let types = app.get("/api/source-types").await;
    let ids: Vec<_> = types
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["id"].as_str().unwrap())
        .collect();
    // The stub type is not offered; the test type is registered by the tests.
    assert_eq!(
        ids,
        ["github", "google-tasks", "phabricator", "bugzilla", "test"]
    );
    assert_eq!(types[0]["name"], "GitHub search");
    assert_eq!(
        types[0]["fields"][0],
        json!({
            "key": "account",
            "label": "GitHub account",
            "kind": "account",
            "required": true,
            "placeholder": null,
            "help": null,
            "provider": "github",
        })
    );
    assert_eq!(types[1]["fields"][0]["kind"], "account");
    assert_eq!(types[1]["fields"][0]["provider"], "google");
    assert_eq!(types[1]["fields"][1]["kind"], "select");
}

fn source_body(fields: Value) -> Value {
    let mut body = json!({
        "type": "test",
        "name": "S",
        "contexts": ["default"],
        "config": { "key": "k" },
    });
    for (key, value) in fields.as_object().unwrap() {
        body[key] = value.clone();
    }
    body
}

#[tokio::test]
async fn validates_sources() {
    let app = start().await;
    for (fields, error) in [
        (json!({ "name": " " }), "Name is required"),
        (json!({ "contexts": [] }), "Choose at least one context"),
        (json!({ "contexts": ["nope"] }), "unknown context `nope`"),
        (json!({ "type": "nope" }), "unknown source type `nope`"),
        (json!({ "config": {} }), "Key is required"),
    ] {
        let (status, body) = app
            .request(Method::POST, "/api/sources", Some(source_body(fields)))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["error"], error);
    }
    assert_eq!(app.get("/api/sources").await, json!([]));
}

#[tokio::test]
async fn updating_source_restarts_it() {
    let app = start_polling("20ms", "20ms").await;
    let id = app.add_source("old", &["default"]).await;
    wait_for(|| app.source("old").fetches() >= 2).await;

    let (status, body) = app
        .request(
            Method::PUT,
            &format!("/api/sources/{id}"),
            Some(source_body(json!({
                "name": "Renamed",
                "config": { "key": "new" },
            }))),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["name"], "Renamed");
    assert_eq!(body["config"], json!({ "key": "new" }));

    wait_for(|| app.source("new").fetches() >= 2).await;
    // The old source is no longer polled.
    let old = app.source("old").fetches();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(app.source("old").fetches() <= old + 1);

    let (status, body) = app
        .request(
            Method::PUT,
            &format!("/api/sources/{id}"),
            Some(source_body(json!({ "type": "github" }))),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "a source's type can't be changed");
}

#[tokio::test]
async fn deleting_source_stops_it_and_removes_items() {
    let app = start_polling("20ms", "20ms").await;
    let id = app.add_source("a", &["default"]).await;
    app.source("a").set_items(vec![item("1", 1, None)]);
    app.refresh("default").await;
    assert_eq!(app.ids("/api/contexts/default/items").await, ["a/1"]);

    let (status, _) = app
        .request(Method::DELETE, &format!("/api/sources/{id}"), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(app.ids("/api/contexts/default/items").await.is_empty());
    assert_eq!(app.get("/api/sources").await, json!([]));

    let fetches = app.source("a").fetches();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(app.source("a").fetches() <= fetches + 1);
    // An in-flight fetch must not resurrect the deleted source's items.
    assert!(
        !db::source_statuses(&app.pool)
            .await
            .unwrap()
            .contains_key(&id)
    );

    let (status, _) = app
        .request(Method::DELETE, &format!("/api/sources/{id}"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn secrets_are_write_only() {
    let app = start().await;
    let (status, body) = app
        .request(
            Method::POST,
            "/api/sources",
            Some(json!({
                "type": "test",
                "name": "Secret",
                "contexts": ["default"],
                "config": { "key": "s", "password": "s3cret" },
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = body["id"].as_str().unwrap().to_string();
    assert_eq!(body["config"], json!({ "key": "s" }));
    assert_eq!(body["secrets_set"], json!(["password"]));

    // Updating without the secret keeps it.
    let (status, body) = app
        .request(
            Method::PUT,
            &format!("/api/sources/{id}"),
            Some(json!({
                "name": "Renamed",
                "contexts": ["default"],
                "config": { "key": "s", "password": "" },
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["secrets_set"], json!(["password"]));
    let stored = settings::get_source(&app.pool, &id).await.unwrap().unwrap();
    assert_eq!(stored.config["password"], "s3cret");
    assert_eq!(stored.name, "Renamed");
}

#[tokio::test]
async fn sources_resume_on_restart() {
    let pool = db::connect_in_memory().await.unwrap();
    let app = start_with("", pool.clone()).await;
    app.add_context("work").await;
    app.add_source("a", &["work"]).await;
    drop(app);

    let app = start_with("", pool).await;
    assert_eq!(app.get("/api/contexts").await.as_array().unwrap().len(), 2);
    // The restarted workspace built its own test source for key `a`.
    wait_for(|| app.source_exists("a") && app.source("a").fetches() >= 1).await;
}

#[tokio::test]
async fn unbuildable_sources_report_an_error() {
    let pool = db::connect_in_memory().await.unwrap();
    settings::ensure_default_context(&pool).await.unwrap();
    settings::save_source(
        &pool,
        &SourceRecord {
            id: "gone".to_string(),
            kind: "removed-type".to_string(),
            name: "Gone".to_string(),
            contexts: vec!["default".to_string()],
            config: json!({}),
            default_due: None,
        },
    )
    .await
    .unwrap();

    let app = start_with("", pool).await;
    let sources = app.get("/api/sources").await;
    assert_eq!(
        sources[0]["status"]["last_error"],
        "unknown source type `removed-type`"
    );
    assert_eq!(sources[0]["icon"], "");
    // Refreshing a source that isn't running is harmless.
    app.refresh("default").await;
}

/// Adds a test source with a default due date, returning its id.
async fn add_due_source(
    app: &common::TestApp,
    key: &str,
    default_due: Value,
) -> (StatusCode, Value) {
    app.request(
        Method::POST,
        "/api/sources",
        Some(json!({
            "type": "test",
            "name": "Due",
            "contexts": ["default"],
            "config": { "key": key },
            "default_due": default_due,
        })),
    )
    .await
}

#[tokio::test]
async fn default_due_dates_count_from_items() {
    let app = start().await;
    let (status, body) =
        add_due_source(&app, "d", json!({ "days": 3, "business_days": false })).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(
        body["default_due"],
        json!({ "days": 3, "business_days": false })
    );
    let id = body["id"].as_str().unwrap().to_string();

    let mut counted = item("counted", 1, None);
    counted.due_from = Some(time(5));
    let mut own = item("own", 2, Some(20));
    own.due_from = Some(time(5));
    let none = item("none", 3, None);
    app.source("d").set_items(vec![counted, own, none]);
    app.refresh("default").await;

    let items = app.get("/api/contexts/default/items").await;
    let due = |id: &str| {
        items
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["id"] == id)
            .unwrap()["due"]
            .clone()
    };
    // Jan 5th 2026 + 3 days.
    assert_eq!(due("counted"), "2026-01-08T00:00:00Z");
    // The source's own due date wins.
    assert_eq!(due("own"), "2026-01-20T00:00:00Z");
    assert_eq!(due("none"), Value::Null);

    // Business days: Monday Jan 5th + 3 is Thursday; Friday Jan 9th + 1 is
    // Monday.
    let mut friday = item("friday", 4, None);
    friday.due_from = Some(time(9));
    let mut counted = item("counted", 1, None);
    counted.due_from = Some(time(5));
    app.source("d").set_items(vec![counted, friday]);
    let (status, body) = app
        .request(
            Method::PUT,
            &format!("/api/sources/{id}"),
            Some(json!({
                "name": "Due",
                "contexts": ["default"],
                "config": { "key": "d" },
                "default_due": { "days": 1, "business_days": true },
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    app.refresh("default").await;
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
            ("counted", "2026-01-06T00:00:00Z"),
            ("friday", "2026-01-12T00:00:00Z")
        ]
    );

    // Turning it off.
    app.request(
        Method::PUT,
        &format!("/api/sources/{id}"),
        Some(json!({ "name": "Due", "contexts": ["default"], "config": { "key": "d" } })),
    )
    .await;
    app.refresh("default").await;
    let items = app.get("/api/contexts/default/items").await;
    assert_eq!(items[0]["due"], Value::Null);
    assert_eq!(app.get("/api/sources").await[0]["default_due"], Value::Null);
}

#[tokio::test]
async fn overrides_beat_default_due_dates() {
    let app = start().await;
    add_due_source(&app, "d", json!({ "days": 3, "business_days": false })).await;
    let mut counted = item("1", 1, None);
    counted.due_from = Some(time(5));
    app.source("d").set_items(vec![counted]);
    app.refresh("default").await;

    let id = app.get("/api/sources").await[0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, _) = app
        .request(
            Method::PUT,
            &format!("/api/items/{id}/1/due"),
            Some(json!({ "due": "2026-02-01T00:00:00Z" })),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let items = app.get("/api/contexts/default/items").await;
    assert_eq!(items[0]["due"], "2026-01-08T00:00:00Z");
    assert_eq!(items[0]["due_override"], "2026-02-01T00:00:00Z");
}

#[tokio::test]
async fn validates_default_due_dates() {
    let app = start().await;
    for days in [0, 366] {
        let (status, body) =
            add_due_source(&app, "d", json!({ "days": days, "business_days": false })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body["error"],
            "The default due date must be between 1 and 365 days"
        );
    }
    let (status, body) = app
        .request(
            Method::POST,
            "/api/sources",
            Some(json!({
                "type": "stub",
                "name": "Stub",
                "contexts": ["default"],
                "config": {},
                "default_due": { "days": 1, "business_days": false },
            })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "Stub sources don't have default due dates");

    let types = app.get("/api/source-types").await;
    let test = types
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == "test")
        .unwrap();
    assert_eq!(test["default_due"], "after testing");
}
