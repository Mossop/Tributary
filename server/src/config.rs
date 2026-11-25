//! Server configuration.
//!
//! Read from a TOML file and overridable by `DASHBOARD_` prefixed environment
//! variables (nested keys separated by `__`, e.g. `DASHBOARD_SERVER__LISTEN`).
//! Contexts and sources are not configured here; they live in the database
//! and are managed through the settings UI.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context as _;
use figment::Figment;
use figment::providers::{Env, Format, Toml};
use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub polling: PollingConfig,
}

/// How often sources are polled. Applies to every source.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PollingConfig {
    /// Polling interval while the dashboard is in use.
    #[serde(with = "humantime_serde")]
    pub interval: Duration,
    /// Polling interval while the dashboard is idle.
    #[serde(with = "humantime_serde")]
    pub idle_interval: Duration,
}

impl Default for PollingConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(5 * 60),
            idle_interval: Duration::from_secs(30 * 60),
        }
    }
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

impl Config {
    /// Loads configuration from a TOML file, if it exists, plus environment
    /// overrides.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        Figment::new()
            .merge(Toml::file(path))
            .merge(Env::prefixed("DASHBOARD_").split("__"))
            .extract()
            .with_context(|| format!("failed to load config from {}", path.display()))
    }

    /// Parses configuration from a TOML string (no environment overrides).
    pub fn from_toml(toml: &str) -> anyhow::Result<Self> {
        Ok(Figment::new().merge(Toml::string(toml)).extract()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_server_config() {
        let config = Config::from_toml(
            r#"
            [server]
            listen = "127.0.0.1:9000"
            active_timeout = "2m"
            "#,
        )
        .unwrap();
        assert_eq!(config.server.listen, "127.0.0.1:9000".parse().unwrap());
        assert_eq!(config.server.active_timeout, Duration::from_secs(120));
        assert_eq!(config.server.database, PathBuf::from("data/dashboard.db"));
    }

    #[test]
    fn parses_polling_config() {
        let config = Config::from_toml(
            r#"
            [polling]
            interval = "1m"
            "#,
        )
        .unwrap();
        assert_eq!(config.polling.interval, Duration::from_secs(60));
        assert_eq!(config.polling.idle_interval, Duration::from_secs(30 * 60));
    }

    #[test]
    fn defaults_when_empty() {
        let config = Config::from_toml("").unwrap();
        assert_eq!(config.server.listen, "0.0.0.0:8080".parse().unwrap());
        assert_eq!(config.polling.interval, Duration::from_secs(5 * 60));
        assert_eq!(config.polling.idle_interval, Duration::from_secs(30 * 60));
    }
}
