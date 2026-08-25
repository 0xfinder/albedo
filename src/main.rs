// Entry point for the Polymarket Telegram Bot

mod bot;
mod config;
mod db;
mod monitoring;
mod utils;

use color_eyre::eyre::Result;
use teloxide::prelude::*;

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

    let _data_handle = monitoring::spawn_data_polling(
        bot.clone(),
        db.clone(),
        config.data_poll_interval,
        config.copy_trade_enabled,
        config.allowed_telegram_ids.clone(),
    );

    let _ws_handle = monitoring::spawn_ws_user_events(
        bot.clone(),
        db.clone(),
        config.encryption_key.clone(),
        config.allowed_telegram_ids.clone(),
    );

    // Start bot dispatcher
    bot::start(
        bot,
        db,
        config.encryption_key.clone(),
        config.allowed_telegram_ids,
    )
    .await?;

    Ok(())
}
