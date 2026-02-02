use color_eyre::eyre::{Context, Result};
use dotenv::dotenv;
use std::env;

pub struct Config {
    pub telegram_token: String,
    pub database_url: String,
    pub polymarket_ws_asset_ids: Vec<String>,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        dotenv().ok();

        let database_url =
            env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://bot.db".to_string());
        let database_url = normalize_database_url(database_url);

        let polymarket_ws_asset_ids = parse_csv_env("POLYMARKET_WS_ASSET_IDS");

        Ok(Self {
            telegram_token: env::var("TELEGRAM_TOKEN").context("TELEGRAM_TOKEN not set")?,
            database_url,
            polymarket_ws_asset_ids,
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

fn parse_csv_env(key: &str) -> Vec<String> {
    match env::var(key) {
        Ok(value) => value
            .split(',')
            .map(|item| item.trim())
            .filter(|item| !item.is_empty())
            .map(str::to_string)
            .collect(),
        Err(_) => Vec::new(),
    }
}
