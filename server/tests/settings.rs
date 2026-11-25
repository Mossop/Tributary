//! Managing contexts and sources at runtime.

mod common;

use std::time::Duration;

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};

use common::{item, start, start_polling, start_with, wait_for};
use tributary::db;
use tributary::settings::{self, SourceRecord};

#[tokio::test]
async fn creates_default_context() {
    let app = start().await;
    assert_eq!(
        app.get("/api/contexts").await,
        json!([{ "id": "default", "name": "Default", "icon": null }])
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
        json!({ "id": "work", "name": "Work", "icon": "work" })
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
            { "id": "personal", "name": "Personal", "icon": null },
            { "id": "work", "name": "Work", "icon": "work" },
        ])
    );
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
    assert_eq!(ids, ["github", "google-tasks", "test"]);
    assert_eq!(types[0]["name"], "GitHub search");
    assert_eq!(
        types[0]["fields"][1],
        json!({
            "key": "token",
            "label": "Access token",
            "kind": "secret",
            "required": true,
            "placeholder": null,
            "help": "A personal access token with read access.",
            "provider": null,
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
                "type": "github",
                "name": "PRs",
                "contexts": ["default"],
                // Unreachable so the test doesn't hit the network.
                "config": { "query": "is:pr", "token": "s3cret", "api_url": "http://127.0.0.1:1" },
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = body["id"].as_str().unwrap().to_string();
    assert_eq!(
        body["config"],
        json!({ "query": "is:pr", "api_url": "http://127.0.0.1:1" })
    );
    assert_eq!(body["secrets_set"], json!(["token"]));
    assert_eq!(body["icon"], "https://github.com/favicon.ico");

    // Updating without the token keeps it.
    let (status, body) = app
        .request(
            Method::PUT,
            &format!("/api/sources/{id}"),
            Some(json!({
                "name": "PRs",
                "contexts": ["default"],
                "config": { "query": "is:issue", "token": "", "api_url": "http://127.0.0.1:1" },
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let stored = settings::get_source(&app.pool, &id).await.unwrap().unwrap();
    assert_eq!(stored.config["token"], "s3cret");
    assert_eq!(stored.config["query"], "is:issue");

    // A secret is still required on creation.
    let (status, body) = app
        .request(
            Method::POST,
            "/api/sources",
            Some(json!({
                "type": "github",
                "name": "PRs",
                "contexts": ["default"],
                "config": { "query": "is:pr" },
            })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "Access token is required");
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
