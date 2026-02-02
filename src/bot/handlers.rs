use teloxide::prelude::*;
use teloxide::utils::command::parse_command;

use crate::db::{self, Db};

const HELP_TEXT: &str = "Available commands:\n\
/start - Start the bot\n\
/help - Show this help message\n\
/track add <address> [label] - Track a wallet\n\
/track list - List tracked wallets\n\
/track remove <address> - Stop tracking a wallet\n\
/track status - Show tracking status\n\
/manage - Switch to manage mode";

pub async fn handle_message(bot: Bot, msg: Message, db: Db, bot_name: String) -> ResponseResult<()> {
    let text = match msg.text() {
        Some(text) => text.to_string(),
        None => return Ok(()),
    };

    let Some((command, args)) = parse_command(text.as_str(), bot_name.as_str()) else {
        return Ok(());
    };

    let Some(user) = msg.from.as_ref() else {
        bot.send_message(msg.chat.id, "This bot only supports direct messages.").await?;
        return Ok(());
    };

    let telegram_id = user.id.0 as i64;
    let chat_id = msg.chat.id.0;
    let user_id = match db::ensure_user(&db, telegram_id, chat_id).await {
        Ok(user_id) => user_id,
        Err(_err) => {
            bot.send_message(msg.chat.id, "Sorry, I couldn't update your profile. Try again soon.")
                .await?;
            return Ok(());
        }
    };

    match command.to_lowercase().as_str() {
        "start" => handle_start(bot, msg).await?,
        "help" => handle_help(bot, msg).await?,
        "track" => handle_track_command(bot, msg, &db, user_id, &args).await?,
        "manage" => handle_manage_mode(bot, msg, &db, user_id).await?,
        _ => {
            bot.send_message(msg.chat.id, "Unknown command. Use /help for available commands.")
                .await?;
        }
    }

    Ok(())
}

async fn handle_start(bot: Bot, msg: Message) -> ResponseResult<()> {
    bot.send_message(msg.chat.id, "Welcome to Polymarket Bot! Use /help to see commands.")
        .await?;
    Ok(())
}

async fn handle_help(bot: Bot, msg: Message) -> ResponseResult<()> {
    bot.send_message(msg.chat.id, HELP_TEXT).await?;
    Ok(())
}

async fn handle_track_command(
    bot: Bot,
    msg: Message,
    db: &Db,
    user_id: i64,
    args: &[&str],
) -> ResponseResult<()> {
    if let Err(_err) = db::set_mode(db, user_id, "track").await {
        bot.send_message(msg.chat.id, "Sorry, I couldn't switch modes. Try again soon.")
            .await?;
        return Ok(());
    }

    if args.is_empty() {
        bot.send_message(msg.chat.id, "Usage: /track add <address> [label] | /track list | /track remove <address> | /track status")
            .await?;
        return Ok(());
    }

    match args[0].to_lowercase().as_str() {
        "add" => handle_track_add(bot, msg, db, user_id, &args[1..]).await?,
        "list" => handle_track_list(bot, msg, db, user_id).await?,
        "remove" => handle_track_remove(bot, msg, db, user_id, &args[1..]).await?,
        "status" => handle_track_status(bot, msg, db, user_id).await?,
        _ => {
            bot.send_message(msg.chat.id, "Unknown /track command. Use /track list or /track add <address>.")
                .await?;
        }
    }

    Ok(())
}

async fn handle_track_add(
    bot: Bot,
    msg: Message,
    db: &Db,
    user_id: i64,
    args: &[&str],
) -> ResponseResult<()> {
    let Some(address) = args.first() else {
        bot.send_message(msg.chat.id, "Usage: /track add <address> [label]").await?;
        return Ok(());
    };

    let wallet_address = normalize_wallet_address(address);
    let label = if args.len() > 1 {
        Some(args[1..].join(" "))
    } else {
        None
    };

    let inserted = match db::add_tracked_wallet(db, user_id, &wallet_address, label.as_deref()).await {
        Ok(inserted) => inserted,
        Err(_err) => {
            bot.send_message(msg.chat.id, "Sorry, I couldn't add that wallet. Try again soon.")
                .await?;
            return Ok(());
        }
    };

    if inserted {
        let response = match label {
            Some(label) => format!("Added wallet {wallet_address} as {label}.",),
            None => format!("Added wallet {wallet_address}.",),
        };
        bot.send_message(msg.chat.id, response).await?;
    } else {
        bot.send_message(msg.chat.id, "That wallet is already being tracked.").await?;
    }

    Ok(())
}

async fn handle_track_list(bot: Bot, msg: Message, db: &Db, user_id: i64) -> ResponseResult<()> {
    let wallets = match db::list_tracked_wallets(db, user_id).await {
        Ok(wallets) => wallets,
        Err(_err) => {
            bot.send_message(msg.chat.id, "Sorry, I couldn't load your tracked wallets.").await?;
            return Ok(());
        }
    };

    if wallets.is_empty() {
        bot.send_message(msg.chat.id, "No tracked wallets yet. Use /track add <address> [label] to add one.")
            .await?;
        return Ok(());
    }

    let mut lines = Vec::with_capacity(wallets.len());
    for wallet in wallets {
        match wallet.label {
            Some(label) => lines.push(format!("- {} ({})", wallet.wallet_address, label)),
            None => lines.push(format!("- {}", wallet.wallet_address)),
        }
    }

    bot.send_message(msg.chat.id, format!("Tracked wallets:\n{}", lines.join("\n")))
        .await?;
    Ok(())
}

async fn handle_track_remove(
    bot: Bot,
    msg: Message,
    db: &Db,
    user_id: i64,
    args: &[&str],
) -> ResponseResult<()> {
    let Some(address) = args.first() else {
        bot.send_message(msg.chat.id, "Usage: /track remove <address>").await?;
        return Ok(());
    };

    let wallet_address = normalize_wallet_address(address);
    let removed = match db::remove_tracked_wallet(db, user_id, &wallet_address).await {
        Ok(removed) => removed,
        Err(_err) => {
            bot.send_message(msg.chat.id, "Sorry, I couldn't remove that wallet. Try again soon.")
                .await?;
            return Ok(());
        }
    };

    if removed {
        bot.send_message(msg.chat.id, format!("Stopped tracking {wallet_address}.")).await?;
    } else {
        bot.send_message(msg.chat.id, "That wallet is not being tracked.").await?;
    }

    Ok(())
}

async fn handle_track_status(bot: Bot, msg: Message, db: &Db, user_id: i64) -> ResponseResult<()> {
    let count = match db::count_tracked_wallets(db, user_id).await {
        Ok(count) => count,
        Err(_err) => {
            bot.send_message(msg.chat.id, "Sorry, I couldn't load tracking status.").await?;
            return Ok(());
        }
    };

    bot.send_message(
        msg.chat.id,
        format!("Tracking {count} wallet(s). Monitoring is not enabled yet."),
    )
    .await?;
    Ok(())
}

async fn handle_manage_mode(bot: Bot, msg: Message, db: &Db, user_id: i64) -> ResponseResult<()> {
    if let Err(_err) = db::set_mode(db, user_id, "manage").await {
        bot.send_message(msg.chat.id, "Sorry, I couldn't switch modes. Try again soon.")
            .await?;
        return Ok(());
    }

    bot.send_message(msg.chat.id, "Manage mode activated. Use /manage auth <private_key> to authenticate a wallet.").await?;
    Ok(())
}

fn normalize_wallet_address(raw: &str) -> String {
    raw.trim().to_lowercase()
}
