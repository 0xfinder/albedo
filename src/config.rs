use color_eyre::eyre::{Context, Result};
use dotenv::dotenv;
use std::env;

pub struct Config {
    pub telegram_token: String,
    pub database_url: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        dotenv().ok();

        Ok(Self {
            telegram_token: env::var("TELEGRAM_TOKEN").context("TELEGRAM_TOKEN not set")?,
            database_url: env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:bot.db".to_string()),
        })
    }
}
