// Entry point for the Polymarket Telegram Bot

mod bot;
mod config;
mod db;
mod monitoring;

use color_eyre::eyre::Result;
use teloxide::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    
    // Initialize configuration
    let config = config::Config::from_env()?;

    // Initialize database
    let db = db::init(&config.database_url).await?;
    
    let _monitor_handle = monitoring::spawn_monitoring(
        db.clone(),
        config.polymarket_ws_asset_ids.clone(),
    );

    // Start bot
    let bot = Bot::new(&config.telegram_token);

    // Start bot dispatcher
    bot::start(bot, db).await?;
    
    Ok(())
}
