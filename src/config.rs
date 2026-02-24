use color_eyre::eyre::{Context, Result};
use dotenv::dotenv;
use std::env;
use std::time::Duration;

use crate::utils::crypto::EncryptionKey;

pub struct Config {
    pub telegram_token: String,
    pub database_url: String,
    pub data_poll_interval: Duration,
    pub encryption_key: Option<EncryptionKey>,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        dotenv().ok();

        let database_url =
            env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://bot.db".to_string());
        let database_url = normalize_database_url(database_url);

        let data_poll_interval =
            parse_data_poll_interval(env::var("POLYMARKET_DATA_POLL_SECONDS").ok());
        let encryption_key = read_encryption_key()?;

        Ok(Self {
            telegram_token: env::var("TELEGRAM_TOKEN").context("TELEGRAM_TOKEN not set")?,
            database_url,
            data_poll_interval,
            encryption_key,
        })
    }
}

fn parse_data_poll_interval(raw: Option<String>) -> Duration {
    const DEFAULT_SECS: f64 = 1.0;

    let Some(value) = raw else {
        return Duration::from_secs_f64(DEFAULT_SECS);
    };

    let seconds = match value.trim().parse::<f64>() {
        Ok(seconds) if seconds.is_finite() && seconds >= 0.0 => seconds,
        _ => return Duration::from_secs_f64(DEFAULT_SECS),
    };

    Duration::try_from_secs_f64(seconds).unwrap_or_else(|_| Duration::from_secs_f64(DEFAULT_SECS))
}

fn normalize_database_url(raw: String) -> String {
    if raw.starts_with("sqlite::") || raw.starts_with("sqlite://") {
        return raw;
    }

    if let Some(stripped) = raw.strip_prefix("sqlite:") {
        return format!("sqlite://{}", stripped);
    }

    if raw.contains("://") {
        return raw;
    }

    format!("sqlite://{}", raw)
}

fn read_encryption_key() -> Result<Option<EncryptionKey>> {
    let value = match env::var("ENCRYPTION_KEY") {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };

    Ok(Some(EncryptionKey::from_hex(&value)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_database_url_sqlite_double_slash() {
        let result = normalize_database_url("sqlite://bot.db".to_string());
        assert_eq!(result, "sqlite://bot.db");
    }

    #[test]
    fn normalize_database_url_sqlite_double_colon() {
        let result = normalize_database_url("sqlite::memory:".to_string());
        assert_eq!(result, "sqlite::memory:");
    }

    #[test]
    fn normalize_database_url_sqlite_single_colon() {
        let result = normalize_database_url("sqlite:bot.db".to_string());
        assert_eq!(result, "sqlite://bot.db");
    }

    #[test]
    fn normalize_database_url_other_scheme() {
        let result = normalize_database_url("postgres://localhost/db".to_string());
        assert_eq!(result, "postgres://localhost/db");
    }

    #[test]
    fn normalize_database_url_bare_path() {
        let result = normalize_database_url("bot.db".to_string());
        assert_eq!(result, "sqlite://bot.db");
    }

    #[test]
    fn parse_data_poll_interval_defaults_to_one_second() {
        assert_eq!(
            parse_data_poll_interval(None),
            Duration::from_secs(1),
        );
    }

    #[test]
    fn parse_data_poll_interval_accepts_fractional_seconds() {
        assert_eq!(
            parse_data_poll_interval(Some("0.5".to_string())),
            Duration::from_millis(500),
        );
    }

    #[test]
    fn parse_data_poll_interval_accepts_zero_for_disable() {
        assert_eq!(
            parse_data_poll_interval(Some("0".to_string())),
            Duration::ZERO,
        );
    }

    #[test]
    fn parse_data_poll_interval_invalid_value_falls_back() {
        assert_eq!(
            parse_data_poll_interval(Some("abc".to_string())),
            Duration::from_secs(1),
        );
    }
}
