pub mod handlers;

use crate::db::Db;
use crate::utils::crypto::EncryptionKey;
use teloxide::{dptree, prelude::*};
use teloxide::types::BotCommand;

pub async fn start(
    bot: Bot,
    db: Db,
    encryption_key: Option<EncryptionKey>,
) -> color_eyre::eyre::Result<()> {
    let me = bot.get_me().await?;
    let bot_name = me.user.username.unwrap_or_default();

    bot.set_my_commands(bot_commands()).await?;

    let handler = dptree::entry()
        .branch(Update::filter_message().endpoint(handlers::handle_message))
        .branch(Update::filter_callback_query().endpoint(handlers::handle_callback));

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![db, bot_name, encryption_key])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;

    Ok(())
}

fn bot_commands() -> Vec<BotCommand> {
    vec![
        BotCommand::new("start", "Open the main menu"),
        BotCommand::new("help", "Show help"),
        BotCommand::new("track", "Open the track menu"),
        BotCommand::new("manage", "Open the manage menu"),
        BotCommand::new("version", "Show the bot version"),
    ]
}
