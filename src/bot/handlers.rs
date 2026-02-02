use teloxide::prelude::*;
use teloxide::utils::command::BotCommands;

use crate::db::{self, Db};

use super::commands::Command;

pub async fn handle_command(bot: Bot, msg: Message, cmd: Command, db: Db) -> ResponseResult<()> {
    let Some(user) = msg.from.as_ref() else {
        bot.send_message(msg.chat.id, "This bot only supports direct messages.").await?;
        return Ok(());
    };

    let telegram_id = user.id.0 as i64;
    let chat_id = msg.chat.id.0;

    if let Err(_err) = db::upsert_user(&db, telegram_id, chat_id).await {
        bot.send_message(msg.chat.id, "Sorry, I couldn't update your profile. Try again soon.")
            .await?;
        return Ok(());
    }

    match cmd {
        Command::Start => handle_start(bot, msg).await?,
        Command::Help => handle_help(bot, msg).await?,
        Command::Track => handle_track_mode(bot, msg, &db, telegram_id).await?,
        Command::Manage => handle_manage_mode(bot, msg, &db, telegram_id).await?,
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

async fn handle_track_mode(bot: Bot, msg: Message, db: &Db, telegram_id: i64) -> ResponseResult<()> {
    if let Err(_err) = db::set_mode(db, telegram_id, "track").await {
        bot.send_message(msg.chat.id, "Sorry, I couldn't switch modes. Try again soon.")
            .await?;
        return Ok(());
    }

    bot.send_message(msg.chat.id, "Track mode activated. Use /track add <address> to add a wallet.").await?;
    Ok(())
}

async fn handle_manage_mode(bot: Bot, msg: Message, db: &Db, telegram_id: i64) -> ResponseResult<()> {
    if let Err(_err) = db::set_mode(db, telegram_id, "manage").await {
        bot.send_message(msg.chat.id, "Sorry, I couldn't switch modes. Try again soon.")
            .await?;
        return Ok(());
    }

    bot.send_message(msg.chat.id, "Manage mode activated. Use /manage auth <private_key> to authenticate a wallet.").await?;
    Ok(())
}
