// Entry point for the Polymarket Telegram Bot

mod bot;
mod config;
mod db;
mod monitoring;
mod utils;

use color_eyre::eyre::Result;
use teloxide::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    
    // Initialize configuration
    let config = config::Config::from_env()?;

    // Initialize database
    let db = db::init(&config.database_url).await?;
    
    // Start bot
    let bot = Bot::new(&config.telegram_token);

    let _data_handle = monitoring::spawn_data_polling(
        bot.clone(),
        db.clone(),
        config.data_poll_seconds,
    );

    let _ws_handle = monitoring::spawn_ws_user_events(
        bot.clone(),
        db.clone(),
        config.encryption_key.clone(),
    );

    // Start bot dispatcher
    bot::start(bot, db, config.encryption_key.clone()).await?;
    
    Ok(())
}
