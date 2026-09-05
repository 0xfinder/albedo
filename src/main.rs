// Entry point for the Polymarket Telegram Bot

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
    println!("albedo {VERSION}");

    // Initialize configuration
    let config = config::Config::from_env()?;

    // Initialize database
    let db = db::init(&config.database_url).await?;

    // Start bot
    let bot = Bot::new(&config.telegram_token);

    if config.allowed_telegram_ids.is_empty() {
        eprintln!(
            "WARNING: ALLOWED_TELEGRAM_IDS is empty - the bot is locked down \
             and no one can interact with it. Add your Telegram user ID in .env."
        );
    }

    let state = Arc::new(AppState {
        bot,
        db,
        config: Arc::new(config),
    });

    let _data_handle = monitoring::spawn_data_polling(state.clone());

    let _ws_handle = monitoring::spawn_ws_user_events(state.clone());

    // Start bot dispatcher
    bot::start(state).await?;

    Ok(())
}
