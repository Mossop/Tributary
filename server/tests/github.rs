use serde_json::json;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use tributary::sources::Source;
use tributary::sources::github::GithubSource;

#[tokio::test]
async fn searches_and_maps_items() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/issues"))
        .and(query_param("q", "is:open author:@me"))
        .and(query_param("per_page", "100"))
        .and(header("authorization", "Bearer secret"))
        .and(header("accept", "application/vnd.github+json"))
        .and(header("x-github-api-version", "2022-11-28"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "total_count": 2,
            "incomplete_results": false,
            "items": [
                {
                    "number": 12,
                    "title": "Fix the thing",
                    "html_url": "https://github.com/owner/repo/pull/12",
                    "repository_url": "https://api.github.com/repos/owner/repo",
                    "updated_at": "2026-03-04T05:06:07Z",
                    "user": { "login": "alice" },
                    "state": "open",
                    "draft": false,
                    "pull_request": {
                        "url": "https://api.github.com/repos/owner/repo/pulls/12",
                        "merged_at": null,
                    },
                },
                {
                    "number": 3,
                    "title": "Broken",
                    "html_url": "https://github.com/owner/other/issues/3",
                    "repository_url": "https://api.github.com/repos/owner/other",
                    "updated_at": "2026-02-01T00:00:00Z",
                    "user": null,
                    "state": "closed",
                    "state_reason": "completed",
                },
            ],
        })))
        .expect(1)
        .mount(&server)
        .await;

    let source = GithubSource::new(&server.uri(), "is:open author:@me", "secret").unwrap();
    assert_eq!(source.icon(), "https://github.com/favicon.ico");

    let items = source.fetch().await.unwrap();
    assert_eq!(items.len(), 2);

    assert_eq!(items[0].id, "owner/repo#12");
    assert_eq!(items[0].title, "Fix the thing");
    assert_eq!(items[0].url, "https://github.com/owner/repo/pull/12");
    assert_eq!(items[0].modified.to_rfc3339(), "2026-03-04T05:06:07+00:00");
    assert_eq!(
        items[0].description.as_deref(),
        Some("owner/repo #12 · PR by alice")
    );
    assert_eq!(items[0].due, None);
    let icon = items[0].icon.as_deref().unwrap();
    assert!(icon.starts_with("data:image/svg+xml,%3Csvg fill='%231a7f37' "));

    assert_eq!(items[1].id, "owner/other#3");
    assert_eq!(
        items[1].description.as_deref(),
        Some("owner/other #3 · Issue")
    );
    let icon = items[1].icon.as_deref().unwrap();
    assert!(icon.starts_with("data:image/svg+xml,%3Csvg fill='%238250df' "));
}

#[tokio::test]
async fn reports_api_errors() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/issues"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "message": "API rate limit exceeded",
        })))
        .mount(&server)
        .await;

    let source = GithubSource::new(&server.uri(), "is:open", "secret").unwrap();
    let error = source.fetch().await.unwrap_err();
    assert_eq!(
        error.to_string(),
        "GitHub returned 403 Forbidden: API rate limit exceeded"
    );
}

#[tokio::test]
async fn reports_malformed_responses() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/issues"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
        .mount(&server)
        .await;

    let source = GithubSource::new(&server.uri(), "is:open", "secret").unwrap();
    assert!(source.fetch().await.is_err());
}
