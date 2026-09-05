//! Albedo: personal Telegram bot for Polymarket wallets and trading.
//!
//! Startup order is config, database, background tasks, then the Telegram
//! dispatcher, which runs until shutdown. Access is fail-closed: an empty
//! allowlist locks the bot down for everyone.

mod bot;
mod config;
mod db;
mod monitoring;
mod state;
mod utils;

use std::sync::Arc;

use color_eyre::eyre::Result;
use teloxide::prelude::*;

use state::AppState;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

pub const VERSION: &str = env!("GIT_VERSION");

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    tracing::info!(version = VERSION, "albedo starting");

    // Initialize configuration
    let config = config::Config::from_env()?;

    // Initialize database
    let db = db::init(&config.database_url).await?;

    // Start bot
    let bot = Bot::new(&config.telegram_token);

    if config.allowed_telegram_ids.is_empty() {
        tracing::warn!(
            "ALLOWED_TELEGRAM_IDS is empty; bot is locked down, add your Telegram user ID in .env"
        );
    }

    let state = Arc::new(AppState {
        bot,
        db,
        config: Arc::new(config),
        data_client: polymarket_client_sdk::data::Client::default(),
    });

    let _data_handle = monitoring::spawn_data_polling(state.clone());

    let _ws_handle = monitoring::spawn_ws_user_events(state.clone());

    // Start bot dispatcher
    bot::start(state).await?;

    Ok(())
}
