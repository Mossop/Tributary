//! Configuration loading and validation.
//!
//! Configuration is read from a TOML file and can be overridden by
//! `DASHBOARD_` prefixed environment variables (nested keys separated by
//! `__`, e.g. `DASHBOARD_SERVER__LISTEN`).

use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context as _, bail};
use figment::Figment;
use figment::providers::{Env, Format, Toml};
use serde::{Deserialize, Serialize};

use crate::sources::github::GithubConfig;
use crate::sources::stub::StubConfig;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub contexts: Vec<ContextConfig>,
    #[serde(default)]
    pub sources: Vec<SourceConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    /// Address to listen on.
    pub listen: SocketAddr,
    /// Path to the SQLite database file.
    pub database: PathBuf,
    /// Directory containing the built frontend.
    pub static_dir: PathBuf,
    /// How long after the last client activity the dashboard is considered idle.
    #[serde(with = "humantime_serde")]
    pub active_timeout: Duration,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen: ([0, 0, 0, 0], 8080).into(),
            database: PathBuf::from("data/dashboard.db"),
            static_dir: PathBuf::from("frontend/dist"),
            active_timeout: Duration::from_secs(5 * 60),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ContextConfig {
    pub id: String,
    pub name: String,
    /// Material Symbol name.
    #[serde(default)]
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SourceConfig {
    #[serde(flatten)]
    pub common: SourceCommon,
    #[serde(flatten)]
    pub kind: SourceKind,
}

/// Settings shared by every source type.
#[derive(Debug, Clone, Deserialize)]
pub struct SourceCommon {
    pub id: String,
    pub name: String,
    /// Ids of the contexts this source is displayed in.
    pub contexts: Vec<String>,
    /// Polling interval while the dashboard is in use.
    #[serde(with = "humantime_serde", default = "default_interval")]
    pub interval: Duration,
    /// Polling interval while the dashboard is idle.
    #[serde(with = "humantime_serde", default = "default_idle_interval")]
    pub idle_interval: Duration,
}

fn default_interval() -> Duration {
    Duration::from_secs(5 * 60)
}

fn default_idle_interval() -> Duration {
    Duration::from_secs(30 * 60)
}

/// Source type specific settings, selected by the `type` key.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SourceKind {
    Github(GithubConfig),
    Stub(StubConfig),
}

impl Config {
    /// Loads configuration from a TOML file plus environment overrides.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let config: Config = Figment::new()
            .merge(Toml::file_exact(path))
            .merge(Env::prefixed("DASHBOARD_").split("__"))
            .extract()
            .with_context(|| format!("failed to load config from {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    /// Parses configuration from a TOML string (no environment overrides).
    pub fn from_toml(toml: &str) -> anyhow::Result<Self> {
        let config: Config = Figment::new().merge(Toml::string(toml)).extract()?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        let mut context_ids = HashSet::new();
        for context in &self.contexts {
            if !context_ids.insert(context.id.as_str()) {
                bail!("duplicate context id `{}`", context.id);
            }
        }

        let mut source_ids = HashSet::new();
        for source in &self.sources {
            let common = &source.common;
            if !source_ids.insert(common.id.as_str()) {
                bail!("duplicate source id `{}`", common.id);
            }
            for context in &common.contexts {
                if !context_ids.contains(context.as_str()) {
                    bail!(
                        "source `{}` refers to unknown context `{context}`",
                        common.id
                    );
                }
            }
        }
        Ok(())
    }

    pub fn context(&self, id: &str) -> Option<&ContextConfig> {
        self.contexts.iter().find(|c| c.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"
        [server]
        listen = "127.0.0.1:9000"
        active_timeout = "2m"

        [[contexts]]
        id = "work"
        name = "Work"
        icon = "work"

        [[contexts]]
        id = "home"
        name = "Home"

        [[sources]]
        id = "prs"
        type = "github"
        name = "My PRs"
        contexts = ["work"]
        interval = "1m"
        query = "is:pr author:@me"

        [[sources]]
        id = "todo"
        type = "stub"
        name = "Todo"
        contexts = ["work", "home"]

        [[sources.items]]
        id = "1"
        title = "Thing"
        url = "https://example.com/1"
        modified = "2026-01-01T00:00:00Z"
    "#;

    #[test]
    fn parses_valid_config() {
        let config = Config::from_toml(VALID).unwrap();
        assert_eq!(config.server.listen, "127.0.0.1:9000".parse().unwrap());
        assert_eq!(config.server.active_timeout, Duration::from_secs(120));
        assert_eq!(config.contexts.len(), 2);
        assert_eq!(config.contexts[0].icon.as_deref(), Some("work"));

        let prs = &config.sources[0];
        assert_eq!(prs.common.interval, Duration::from_secs(60));
        assert_eq!(prs.common.idle_interval, default_idle_interval());
        match &prs.kind {
            SourceKind::Github(github) => assert_eq!(github.query, "is:pr author:@me"),
            other => panic!("unexpected kind {other:?}"),
        }

        match &config.sources[1].kind {
            SourceKind::Stub(stub) => assert_eq!(stub.items.len(), 1),
            other => panic!("unexpected kind {other:?}"),
        }
    }

    #[test]
    fn defaults_when_empty() {
        let config = Config::from_toml("").unwrap();
        assert_eq!(config.server.listen, "0.0.0.0:8080".parse().unwrap());
        assert!(config.contexts.is_empty());
        assert!(config.sources.is_empty());
    }

    #[test]
    fn rejects_duplicate_context() {
        let err = Config::from_toml(
            r#"
            [[contexts]]
            id = "a"
            name = "A"
            [[contexts]]
            id = "a"
            name = "B"
            "#,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("duplicate context id `a`"),
            "{err}"
        );
    }

    #[test]
    fn rejects_duplicate_source() {
        let err = Config::from_toml(
            r#"
            [[contexts]]
            id = "a"
            name = "A"
            [[sources]]
            id = "s"
            type = "stub"
            name = "S"
            contexts = ["a"]
            [[sources]]
            id = "s"
            type = "stub"
            name = "S"
            contexts = ["a"]
            "#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("duplicate source id `s`"), "{err}");
    }

    #[test]
    fn rejects_unknown_context() {
        let err = Config::from_toml(
            r#"
            [[sources]]
            id = "s"
            type = "stub"
            name = "S"
            contexts = ["nope"]
            "#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("unknown context `nope`"), "{err}");
    }

    #[test]
    fn rejects_unknown_source_type() {
        let err = Config::from_toml(
            r#"
            [[sources]]
            id = "s"
            type = "carrier-pigeon"
            name = "S"
            contexts = []
            "#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("carrier-pigeon"), "{err}");
    }
}
