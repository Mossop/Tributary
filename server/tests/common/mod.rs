#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use chrono::{DateTime, TimeZone, Utc};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

use dashboard::config::Config;
use dashboard::sources::{Source, SourceError, SourceItem};
use dashboard::{Workspace, api, db};

/// A source whose results are controlled by the test.
pub struct TestSource {
    result: Mutex<Result<Vec<SourceItem>, String>>,
    fetches: AtomicUsize,
}

impl TestSource {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            result: Mutex::new(Ok(Vec::new())),
            fetches: AtomicUsize::new(0),
        })
    }

    pub fn set_items(&self, items: Vec<SourceItem>) {
        *self.result.lock().unwrap() = Ok(items);
    }

    pub fn set_error(&self, error: &str) {
        *self.result.lock().unwrap() = Err(error.to_string());
    }

    pub fn fetches(&self) -> usize {
        self.fetches.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Source for TestSource {
    fn icon(&self) -> String {
        "symbol:science".to_string()
    }

    async fn fetch(&self) -> Result<Vec<SourceItem>, SourceError> {
        self.fetches.fetch_add(1, Ordering::SeqCst);
        self.result
            .lock()
            .unwrap()
            .clone()
            .map_err(SourceError::Message)
    }
}

pub fn time(day: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 1, day, 0, 0, 0).unwrap()
}

pub fn item(id: &str, modified: u32, due: Option<u32>) -> SourceItem {
    SourceItem {
        id: id.to_string(),
        title: format!("Item {id}"),
        url: format!("https://example.com/{id}"),
        icon: None,
        modified: time(modified),
        description: None,
        due: due.map(time),
    }
}

pub struct TestApp {
    pub router: Router,
    pub workspace: Arc<Workspace>,
    pub sources: HashMap<String, Arc<TestSource>>,
}

/// Starts an app from the given config, replacing every configured source
/// with a [`TestSource`].
pub async fn start(toml: &str) -> TestApp {
    let config = Config::from_toml(toml).unwrap();
    let pool = db::connect_in_memory().await.unwrap();

    let mut sources = HashMap::new();
    let mut entries: Vec<(_, Arc<dyn Source>)> = Vec::new();
    for source in &config.sources {
        let test_source = TestSource::new();
        sources.insert(source.common.id.clone(), test_source.clone());
        entries.push((source.common.clone(), test_source));
    }

    let workspace = Workspace::start(config, entries, pool);
    let router = api::router(workspace.clone());
    TestApp {
        router,
        workspace,
        sources,
    }
}

impl TestApp {
    pub fn source(&self, id: &str) -> &TestSource {
        &self.sources[id]
    }

    pub async fn request(
        &self,
        method: Method,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder().method(method).uri(uri);
        let body = match body {
            Some(body) => {
                request = request.header("content-type", "application/json");
                Body::from(body.to_string())
            }
            None => Body::empty(),
        };
        let response = self
            .router
            .clone()
            .oneshot(request.body(body).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes)
                .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()))
        };
        (status, value)
    }

    pub async fn get(&self, uri: &str) -> Value {
        let (status, value) = self.request(Method::GET, uri, None).await;
        assert_eq!(status, StatusCode::OK, "GET {uri}: {value}");
        value
    }

    pub async fn refresh(&self, context: &str) {
        let (status, _) = self
            .request(
                Method::POST,
                &format!("/api/contexts/{context}/refresh"),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }

    /// Ids (`source/item`) of the items returned by a list endpoint.
    pub async fn ids(&self, uri: &str) -> Vec<String> {
        self.get(uri)
            .await
            .as_array()
            .unwrap()
            .iter()
            .map(|i| {
                format!(
                    "{}/{}",
                    i["source_id"].as_str().unwrap(),
                    i["id"].as_str().unwrap()
                )
            })
            .collect()
    }
}

/// Polls until `condition` is true, panicking after a timeout.
pub async fn wait_for(mut condition: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(tokio::time::Instant::now() < deadline, "timed out waiting");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
