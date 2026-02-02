// Entry point for the Polymarket Telegram Bot

mod bot;
mod config;

use color_eyre::eyre::Result;
use teloxide::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    
    // Initialize configuration
    let config = config::Config::from_env()?;
    
    // Start bot
    let bot = Bot::new(&config.telegram_token);

    // Start bot dispatcher
    bot::start(bot).await?;
    
    Ok(())
}
