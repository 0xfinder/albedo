use teloxide::prelude::*;
use teloxide::utils::command::BotCommands;

use super::commands::Command;

pub async fn handle_command(bot: Bot, msg: Message, cmd: Command) -> ResponseResult<()> {
    match cmd {
        Command::Start => handle_start(bot, msg).await?,
        Command::Help => handle_help(bot, msg).await?,
        Command::Track => handle_track_mode(bot, msg).await?,
        Command::Manage => handle_manage_mode(bot, msg).await?,
    }
    Ok(())
}

async fn handle_start(bot: Bot, msg: Message) -> ResponseResult<()> {
    bot.send_message(msg.chat.id, "Welcome to Polymarket Bot! Use /track or /manage to get started.").await?;
    Ok(())
}

async fn handle_help(bot: Bot, msg: Message) -> ResponseResult<()> {
    bot.send_message(msg.chat.id, Command::descriptions().to_string()).await?;
    Ok(())
}

async fn handle_track_mode(bot: Bot, msg: Message) -> ResponseResult<()> {
    bot.send_message(msg.chat.id, "Track mode activated. Use /track add <address> to add a wallet.").await?;
    Ok(())
}

async fn handle_manage_mode(bot: Bot, msg: Message) -> ResponseResult<()> {
    bot.send_message(msg.chat.id, "Manage mode activated. Use /manage auth <private_key> to authenticate a wallet.").await?;
    Ok(())
}
