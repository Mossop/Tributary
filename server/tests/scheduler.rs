//! Polling behaviour. These use real time with short intervals.

mod common;

use std::time::Duration;

use axum::http::Method;

use common::{item, start, wait_for};

fn config(interval: &str, idle_interval: &str) -> String {
    format!(
        r#"
        [[contexts]]
        id = "work"
        name = "Work"

        [[sources]]
        id = "a"
        type = "stub"
        name = "A"
        contexts = ["work"]
        interval = "{interval}"
        idle_interval = "{idle_interval}"
        "#
    )
}

#[tokio::test]
async fn fetches_on_start() {
    let app = start(&config("1h", "1h")).await;
    app.source("a").set_items(vec![item("1", 1, None)]);
    wait_for(|| app.source("a").fetches() >= 1).await;
}

#[tokio::test]
async fn polls_at_idle_interval_when_idle() {
    let app = start(&config("1h", "20ms")).await;
    wait_for(|| app.source("a").fetches() >= 4).await;
}

#[tokio::test]
async fn polls_at_active_interval_when_active() {
    let app = start(&config("20ms", "1h")).await;
    wait_for(|| app.source("a").fetches() >= 1).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    // Idle: only the initial fetch.
    assert_eq!(app.source("a").fetches(), 1);

    app.request(Method::POST, "/api/activity", None).await;
    wait_for(|| app.source("a").fetches() >= 5).await;
}

#[tokio::test]
async fn concurrent_refreshes_complete() {
    let app = start(&config("1h", "1h")).await;
    let refreshes = (0..5).map(|_| app.refresh("work"));
    tokio::time::timeout(Duration::from_secs(5), futures::future::join_all(refreshes))
        .await
        .expect("refreshes should complete");
}
