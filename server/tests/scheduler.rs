//! Polling behaviour. These use real time with short intervals.

mod common;

use std::time::Duration;

use axum::http::Method;

use common::{start, start_polling, wait_for};

#[tokio::test]
async fn fetches_when_added() {
    let app = start().await;
    app.add_source("a", &["default"]).await;
    wait_for(|| app.source("a").fetches() >= 1).await;
}

#[tokio::test]
async fn polls_at_idle_interval_when_idle() {
    let app = start_polling("1h", "20ms").await;
    app.add_source("a", &["default"]).await;
    wait_for(|| app.source("a").fetches() >= 4).await;
}

#[tokio::test]
async fn polls_at_active_interval_when_active() {
    let app = start_polling("20ms", "1h").await;
    app.add_source("a", &["default"]).await;
    wait_for(|| app.source("a").fetches() >= 1).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    // Idle: only the initial fetch.
    assert_eq!(app.source("a").fetches(), 1);

    app.request(Method::POST, "/api/activity", None).await;
    wait_for(|| app.source("a").fetches() >= 5).await;
}

#[tokio::test]
async fn concurrent_refreshes_complete() {
    let app = start().await;
    app.add_source("a", &["default"]).await;
    let refreshes = (0..5).map(|_| app.refresh("default"));
    tokio::time::timeout(Duration::from_secs(5), futures::future::join_all(refreshes))
        .await
        .expect("refreshes should complete");
}
