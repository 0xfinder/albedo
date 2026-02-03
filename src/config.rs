use color_eyre::eyre::{Context, Result};
use dotenv::dotenv;
use std::env;

use crate::utils::crypto::EncryptionKey;

pub struct Config {
    pub telegram_token: String,
    pub database_url: String,
    pub data_poll_seconds: u64,
    pub polymarket_private_key: Option<String>,
    pub encryption_key: Option<EncryptionKey>,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        dotenv().ok();

        let database_url =
            env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://bot.db".to_string());
        let database_url = normalize_database_url(database_url);

        let data_poll_seconds = env::var("POLYMARKET_DATA_POLL_SECONDS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(1);
        let polymarket_private_key = env::var("POLYMARKET_PRIVATE_KEY").ok();
        let encryption_key = read_encryption_key()?;

        Ok(Self {
            telegram_token: env::var("TELEGRAM_TOKEN").context("TELEGRAM_TOKEN not set")?,
            database_url,
            data_poll_seconds,
            polymarket_private_key,
            encryption_key,
        })
    }
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
}
