use color_eyre::eyre::{Context, Result};
use dotenv::dotenv;
use std::env;

pub struct Config {
    pub telegram_token: String,
    pub database_url: String,
    pub data_poll_seconds: u64,
    pub ws_credentials: Option<WsCredentialsConfig>,
}

#[derive(Clone, Debug)]
pub struct WsCredentialsConfig {
    pub api_key: String,
    pub api_secret: String,
    pub api_passphrase: String,
    pub address: String,
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
        let ws_credentials = read_ws_credentials();

        Ok(Self {
            telegram_token: env::var("TELEGRAM_TOKEN").context("TELEGRAM_TOKEN not set")?,
            database_url,
            data_poll_seconds,
            ws_credentials,
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

fn read_ws_credentials() -> Option<WsCredentialsConfig> {
    let api_key = env::var("POLYMARKET_API_KEY").ok()?;
    let api_secret = env::var("POLYMARKET_API_SECRET").ok()?;
    let api_passphrase = env::var("POLYMARKET_API_PASSPHRASE").ok()?;
    let address = env::var("POLYMARKET_ADDRESS").ok()?;

    Some(WsCredentialsConfig {
        api_key,
        api_secret,
        api_passphrase,
        address,
    })
}
