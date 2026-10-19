//! Service configuration, read from `PAYMENTS_*` environment variables.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use tracing::Level;

use crate::provider::RetryPolicy;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub listen_addr: SocketAddr,
    pub data_dir: PathBuf,
    pub log_level: Level,
    pub provider_url: String,
    pub provider_retry: RetryPolicy,
    pub cache_per_tenant: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("{0} is required")]
    Missing(&'static str),
    #[error("{name} has an invalid value {value:?}")]
    Invalid { name: &'static str, value: String },
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// Reads the configuration through `lookup`, which returns a variable's
    /// value if it is set.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let required = |name| lookup(name).ok_or(ConfigError::Missing(name));
        Ok(Self {
            listen_addr: parsed(&lookup, "PAYMENTS_LISTEN_ADDR", "127.0.0.1:8080")?,
            data_dir: parsed(&lookup, "PAYMENTS_DATA_DIR", "data")?,
            log_level: parsed(&lookup, "PAYMENTS_LOG_LEVEL", "info")?,
            provider_url: required("PAYMENTS_PROVIDER_URL")?,
            provider_retry: RetryPolicy {
                attempt_timeout: millis(&lookup, "PAYMENTS_PROVIDER_TIMEOUT_MS", 2_000)?,
                max_attempts: parsed(&lookup, "PAYMENTS_PROVIDER_MAX_ATTEMPTS", "3")?,
                backoff: millis(&lookup, "PAYMENTS_PROVIDER_BACKOFF_MS", 100)?,
            },
            cache_per_tenant: parsed(&lookup, "PAYMENTS_CACHE_PER_TENANT", "1024")?,
        })
    }
}

fn parsed<T: FromStr>(
    lookup: &impl Fn(&str) -> Option<String>,
    name: &'static str,
    default: &str,
) -> Result<T, ConfigError> {
    let value = lookup(name).unwrap_or_else(|| default.to_owned());
    value
        .parse()
        .map_err(|_| ConfigError::Invalid { name, value })
}

fn millis(
    lookup: &impl Fn(&str) -> Option<String>,
    name: &'static str,
    default: u64,
) -> Result<Duration, ConfigError> {
    parsed(lookup, name, &default.to_string()).map(Duration::from_millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(vars: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let var = |name: &str| vars.iter().find(|(key, _)| *key == name);
        Config::from_lookup(|name| var(name).map(|(_, value)| (*value).to_owned()))
    }

    #[test]
    fn defaults_apply_when_only_the_provider_url_is_set() {
        let config = load(&[("PAYMENTS_PROVIDER_URL", "http://provider.internal")]).unwrap();

        assert_eq!(config.listen_addr, "127.0.0.1:8080".parse().unwrap());
        assert_eq!(config.log_level, Level::INFO);
        assert_eq!(config.provider_url, "http://provider.internal");
        assert_eq!(
            config.provider_retry.attempt_timeout,
            Duration::from_secs(2)
        );
        assert_eq!(config.provider_retry.max_attempts.get(), 3);
    }

    #[test]
    fn missing_and_invalid_values_are_named() {
        let missing = load(&[]).unwrap_err();
        let invalid = load(&[
            ("PAYMENTS_PROVIDER_URL", "http://p"),
            ("PAYMENTS_PROVIDER_MAX_ATTEMPTS", "0"),
        ]);

        assert_eq!(missing.to_string(), "PAYMENTS_PROVIDER_URL is required");
        assert_eq!(
            invalid.unwrap_err().to_string(),
            r#"PAYMENTS_PROVIDER_MAX_ATTEMPTS has an invalid value "0""#
        );
    }
}
