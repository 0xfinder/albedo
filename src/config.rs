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
    pub copy_trade_enabled: bool,
    /// Telegram user IDs allowed to interact with the bot. Empty locks the
    /// bot down; there is no open-access mode.
    pub allowed_telegram_ids: Vec<i64>,
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
        let copy_trade_enabled = parse_enabled_flag(env::var("COPY_TRADE_ENABLED").ok());
        let allowed_telegram_ids =
            parse_allowed_telegram_ids(env::var("ALLOWED_TELEGRAM_IDS").ok());

        Ok(Self {
            telegram_token: env::var("TELEGRAM_TOKEN").context("TELEGRAM_TOKEN not set")?,
            database_url,
            data_poll_interval,
            encryption_key,
            copy_trade_enabled,
            allowed_telegram_ids,
        })
    }
}

// Feature flags default to off; only explicit truthy values enable them.
fn parse_enabled_flag(raw: Option<String>) -> bool {
    matches!(raw.as_deref().map(str::trim), Some(v) if v.eq_ignore_ascii_case("1") || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes"))
}

// Fail closed: an unset or empty list locks the bot down entirely. Only
// explicit IDs grant access.
fn parse_allowed_telegram_ids(raw: Option<String>) -> Vec<i64> {
    raw.unwrap_or_default()
        .split(',')
        .filter_map(|part| part.trim().parse::<i64>().ok())
        .collect()
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
        assert_eq!(parse_data_poll_interval(None), Duration::from_secs(1),);
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

    #[test]
    fn parse_enabled_flag_defaults_to_off() {
        assert!(!parse_enabled_flag(None));
        assert!(!parse_enabled_flag(Some(String::new())));
        assert!(!parse_enabled_flag(Some("0".to_string())));
    }

    #[test]
    fn parse_enabled_flag_accepts_truthy_values() {
        assert!(parse_enabled_flag(Some("1".to_string())));
        assert!(parse_enabled_flag(Some("true".to_string())));
        assert!(parse_enabled_flag(Some("YES".to_string())));
        assert!(parse_enabled_flag(Some(" yes ".to_string())));
    }

    #[test]
    fn parse_allowed_telegram_ids_unset_locks_down() {
        assert!(parse_allowed_telegram_ids(None).is_empty());
    }

    #[test]
    fn parse_allowed_telegram_ids_parses_csv() {
        assert_eq!(
            parse_allowed_telegram_ids(Some("123, 456,789".to_string())),
            vec![123, 456, 789],
        );
    }

    #[test]
    fn parse_allowed_telegram_ids_skips_invalid_entries() {
        assert_eq!(
            parse_allowed_telegram_ids(Some("123, abc, 456".to_string())),
            vec![123, 456],
        );
    }

    #[test]
    fn parse_allowed_telegram_ids_empty_locks_down() {
        assert!(parse_allowed_telegram_ids(Some(String::new())).is_empty());
    }
}
