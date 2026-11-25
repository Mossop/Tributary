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
use serde_json::{Value, json};
use sqlx::SqlitePool;
use tower::ServiceExt;

use dashboard::config::Config;
use dashboard::sources::{Field, FieldKind, Registry, Source, SourceError, SourceItem, SourceType};
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

type TestSources = Arc<Mutex<HashMap<String, Arc<TestSource>>>>;

/// A `test` source type whose config is `{ "key": "<name>" }`. Each key gets
/// its own [`TestSource`], shared between builds so tests can control it.
fn test_source_type(sources: TestSources) -> SourceType {
    SourceType::new(
        "test",
        "Test",
        vec![Field {
            key: "key",
            label: "Key",
            kind: FieldKind::Text,
            required: true,
            placeholder: None,
            help: None,
        }],
        move |config| {
            let key = config["key"].as_str().unwrap().to_string();
            let source = sources
                .lock()
                .unwrap()
                .entry(key)
                .or_insert_with(TestSource::new)
                .clone();
            Ok(source as Arc<dyn Source>)
        },
    )
}

pub struct TestApp {
    pub router: Router,
    pub workspace: Arc<Workspace>,
    pub pool: SqlitePool,
    test_sources: TestSources,
    /// Test source key to source id.
    ids: Mutex<HashMap<String, String>>,
}

/// Starts an app with long polling intervals, so sources only fetch when
/// added or refreshed.
pub async fn start() -> TestApp {
    start_polling("1h", "1h").await
}

/// Starts an app polling at the given intervals (humantime strings).
pub async fn start_polling(interval: &str, idle_interval: &str) -> TestApp {
    start_with(
        &format!("[polling]\ninterval = \"{interval}\"\nidle_interval = \"{idle_interval}\"\n"),
        db::connect_in_memory().await.unwrap(),
    )
    .await
}

/// Starts an app with the given server config and database.
pub async fn start_with(toml: &str, pool: SqlitePool) -> TestApp {
    let config = Config::from_toml(toml).unwrap();
    let test_sources = TestSources::default();
    let mut registry = Registry::builtin();
    registry.register(test_source_type(test_sources.clone()));

    let workspace = Workspace::start(config, pool.clone(), registry)
        .await
        .unwrap();
    let router = api::router(workspace.clone());
    TestApp {
        router,
        workspace,
        pool,
        test_sources,
        ids: Mutex::new(HashMap::new()),
    }
}

impl TestApp {
    /// The test source with the given key.
    pub fn source(&self, key: &str) -> Arc<TestSource> {
        self.test_sources.lock().unwrap()[key].clone()
    }

    pub fn source_exists(&self, key: &str) -> bool {
        self.test_sources.lock().unwrap().contains_key(key)
    }

    /// The id of the source created for a test source key.
    pub fn id(&self, key: &str) -> String {
        self.ids.lock().unwrap()[key].clone()
    }

    pub async fn add_context(&self, id: &str) {
        let (status, body) = self
            .request(
                Method::POST,
                "/api/contexts",
                Some(json!({ "id": id, "name": id.to_uppercase() })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }

    /// Adds a test source shown in `contexts`, returning its id.
    pub async fn add_source(&self, key: &str, contexts: &[&str]) -> String {
        let (status, body) = self
            .request(
                Method::POST,
                "/api/sources",
                Some(json!({
                    "type": "test",
                    "name": format!("Source {key}"),
                    "contexts": contexts,
                    "config": { "key": key },
                })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        let id = body["id"].as_str().unwrap().to_string();
        self.ids.lock().unwrap().insert(key.to_string(), id.clone());
        id
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

    /// Ids (`source key/item`) of the items returned by a list endpoint.
    pub async fn ids(&self, uri: &str) -> Vec<String> {
        let keys: HashMap<String, String> = self
            .ids
            .lock()
            .unwrap()
            .iter()
            .map(|(key, id)| (id.clone(), key.clone()))
            .collect();
        self.get(uri)
            .await
            .as_array()
            .unwrap()
            .iter()
            .map(|i| {
                let source = i["source_id"].as_str().unwrap();
                format!(
                    "{}/{}",
                    keys.get(source).map(String::as_str).unwrap_or(source),
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
