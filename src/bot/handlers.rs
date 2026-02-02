use teloxide::prelude::*;
use teloxide::types::{
    CallbackQuery, InlineKeyboardButton, InlineKeyboardMarkup, KeyboardRemove,
};
use teloxide::utils::command::parse_command;

use crate::db::{self, Db};

const HELP_TEXT: &str = "Available commands:\n\
/start - Start the bot\n\
/help - Show this help message";

const ACTION_TRACK_ADD: &str = "track_add";
const ACTION_TRACK_REMOVE: &str = "track_remove";

pub async fn handle_message(bot: Bot, msg: Message, db: Db, bot_name: String) -> ResponseResult<()> {
    let text = match msg.text() {
        Some(text) => text.to_string(),
        None => return Ok(()),
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

    if let Some((command, _args)) = parse_incoming_command(text.as_str(), bot_name.as_str()) {
        let _ = db::clear_pending_action(&db, user_id).await;
        return handle_top_level_command(bot, msg, &db, user_id, command.as_str()).await;
    }

    match db::get_pending_action(&db, user_id).await {
        Ok(Some(action)) => {
            return handle_pending_action(bot, msg, &db, user_id, action.as_str(), text.as_str())
                .await;
        }
        Ok(None) => {}
        Err(_err) => {
            bot.send_message(msg.chat.id, "Sorry, I couldn't read your session state.").await?;
            return Ok(());
        }
    }

    bot.send_message(msg.chat.id, "Use /start to open the menu.").await?;
    Ok(())
}

pub async fn handle_callback(bot: Bot, query: CallbackQuery, db: Db) -> ResponseResult<()> {
    let Some(data) = query.data.clone() else {
        return Ok(());
    };

    let chat_id = query
        .message
        .as_ref()
        .map(|message| message.chat().id.0)
        .unwrap_or(query.from.id.0 as i64);

    let user_id = match db::ensure_user(&db, query.from.id.0 as i64, chat_id).await {
        Ok(user_id) => user_id,
        Err(_err) => {
            bot.answer_callback_query(query.id).await?;
            return Ok(());
        }
    };

    match data.as_str() {
        "menu:main" => {
            let _ = db::clear_pending_action(&db, user_id).await;
            send_callback_menu(&bot, &query, "Choose a mode:", main_menu_markup()).await?;
        }
        "menu:track" => {
            let _ = db::clear_pending_action(&db, user_id).await;
            let _ = db::set_mode(&db, user_id, "track").await;
            send_callback_menu(&bot, &query, "Track menu:", track_menu_markup()).await?;
        }
        "menu:manage" => {
            let _ = db::clear_pending_action(&db, user_id).await;
            let _ = db::set_mode(&db, user_id, "manage").await;
            send_callback_menu(&bot, &query, "Manage menu (coming soon):", manage_menu_markup()).await?;
        }
        "track:add" => {
            let _ = db::set_pending_action(&db, user_id, ACTION_TRACK_ADD).await;
            send_callback_menu(
                &bot,
                &query,
                "Send the wallet address to track.",
                cancel_menu_markup(),
            )
            .await?;
        }
        "track:remove" => {
            let _ = db::set_pending_action(&db, user_id, ACTION_TRACK_REMOVE).await;
            send_callback_menu(
                &bot,
                &query,
                "Send the wallet address to remove.",
                cancel_menu_markup(),
            )
            .await?;
        }
        "track:list" => {
            let _ = db::clear_pending_action(&db, user_id).await;
            let chat_id = query
                .message
                .as_ref()
                .map(|message| message.chat().id)
                .unwrap_or(ChatId(query.from.id.0 as i64));
            send_tracked_wallets(&bot, chat_id, &db, user_id).await?;
            bot.answer_callback_query(query.id).await?;
        }
        "track:status" => {
            let _ = db::clear_pending_action(&db, user_id).await;
            let chat_id = query
                .message
                .as_ref()
                .map(|message| message.chat().id)
                .unwrap_or(ChatId(query.from.id.0 as i64));
            send_track_status(&bot, chat_id, &db, user_id).await?;
            bot.answer_callback_query(query.id).await?;
        }
        "action:cancel" => {
            let _ = db::clear_pending_action(&db, user_id).await;
            send_callback_menu(&bot, &query, "Track menu:", track_menu_markup()).await?;
        }
        _ => {
            bot.answer_callback_query(query.id).await?;
        }
    }

    Ok(())
}
async fn handle_top_level_command(
    bot: Bot,
    msg: Message,
    db: &Db,
    user_id: i64,
    command: &str,
) -> ResponseResult<()> {
    match command {
        "start" => handle_start(bot, msg).await?,
        "help" => handle_help(bot, msg).await?,
        "track" => {
            let _ = db::set_mode(db, user_id, "track").await;
            bot.send_message(msg.chat.id, "Track menu:")
                .reply_markup(track_menu_markup())
                .await?;
        }
        "manage" => {
            let _ = db::set_mode(db, user_id, "manage").await;
            bot.send_message(msg.chat.id, "Manage menu (coming soon):")
                .reply_markup(manage_menu_markup())
                .await?;
        }
        _ => {
            bot.send_message(msg.chat.id, "Unknown command. Use /start for the menu.")
                .await?;
        }
    }

    Ok(())
}

async fn handle_start(bot: Bot, msg: Message) -> ResponseResult<()> {
    bot.send_message(msg.chat.id, "Menu updated.")
        .reply_markup(KeyboardRemove::new())
        .await?;

    bot.send_message(msg.chat.id, "Choose a mode:")
        .reply_markup(main_menu_markup())
        .await?;
    Ok(())
}

async fn handle_help(bot: Bot, msg: Message) -> ResponseResult<()> {
    bot.send_message(msg.chat.id, HELP_TEXT).await?;
    Ok(())
}

async fn handle_pending_action(
    bot: Bot,
    msg: Message,
    db: &Db,
    user_id: i64,
    action: &str,
    input: &str,
) -> ResponseResult<()> {
    match action {
        ACTION_TRACK_ADD => {
            if !is_valid_wallet_address(input) {
                bot.send_message(msg.chat.id, "That wallet address looks invalid. Expected 0x + 40 hex characters.")
                    .await?;
                return Ok(());
            }

            let wallet_address = normalize_wallet_address(input);
            let inserted = match db::add_tracked_wallet(db, user_id, &wallet_address, None).await
            {
                Ok(inserted) => inserted,
                Err(_err) => {
                    bot.send_message(msg.chat.id, "Sorry, I couldn't add that wallet. Try again soon.")
                        .await?;
                    return Ok(());
                }
            };

            let _ = db::clear_pending_action(db, user_id).await;

            if inserted {
                bot.send_message(msg.chat.id, format!("Added wallet {wallet_address}."))
                    .await?;
            } else {
                bot.send_message(msg.chat.id, "That wallet is already being tracked.")
                    .await?;
            }

            bot.send_message(msg.chat.id, "Track menu:")
                .reply_markup(track_menu_markup())
                .await?;
        }
        ACTION_TRACK_REMOVE => {
            if !is_valid_wallet_address(input) {
                bot.send_message(msg.chat.id, "That wallet address looks invalid. Expected 0x + 40 hex characters.")
                    .await?;
                return Ok(());
            }

            let wallet_address = normalize_wallet_address(input);
            let removed = match db::remove_tracked_wallet(db, user_id, &wallet_address).await {
                Ok(removed) => removed,
                Err(_err) => {
                    bot.send_message(msg.chat.id, "Sorry, I couldn't remove that wallet. Try again soon.")
                        .await?;
                    return Ok(());
                }
            };

            let _ = db::clear_pending_action(db, user_id).await;

            if removed {
                bot.send_message(msg.chat.id, format!("Stopped tracking {wallet_address}."))
                    .await?;
            } else {
                bot.send_message(msg.chat.id, "That wallet is not being tracked.")
                    .await?;
            }

            bot.send_message(msg.chat.id, "Track menu:")
                .reply_markup(track_menu_markup())
                .await?;
        }
        _ => {
            let _ = db::clear_pending_action(db, user_id).await;
            bot.send_message(msg.chat.id, "That action expired. Use /start to open the menu.")
                .await?;
        }
    }

    Ok(())
}

fn normalize_wallet_address(raw: &str) -> String {
    raw.trim().to_lowercase()
}

fn is_valid_wallet_address(raw: &str) -> bool {
    let trimmed = raw.trim();
    if !trimmed.starts_with("0x") {
        return false;
    }

    if trimmed.len() != 42 {
        return false;
    }

    trimmed[2..].chars().all(|c| c.is_ascii_hexdigit())
}

fn parse_incoming_command(text: &str, bot_name: &str) -> Option<(String, Vec<String>)> {
    if let Some((command, args)) = parse_command(text, bot_name) {
        return Some((
            command.to_lowercase(),
            args.into_iter().map(str::to_string).collect(),
        ));
    }

    let mut parts = text.split_whitespace();
    let command = parts.next()?.to_lowercase();
    let args: Vec<String> = parts.map(str::to_string).collect();

    match command.as_str() {
        "start" | "help" | "track" | "manage" => Some((command, args)),
        _ => None,
    }
}

fn main_menu_markup() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![
        InlineKeyboardButton::callback("Track", "menu:track"),
        InlineKeyboardButton::callback("Manage", "menu:manage"),
    ]])
}

fn track_menu_markup() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![
            InlineKeyboardButton::callback("Add address", "track:add"),
            InlineKeyboardButton::callback("Remove address", "track:remove"),
        ],
        vec![
            InlineKeyboardButton::callback("View all", "track:list"),
            InlineKeyboardButton::callback("Status", "track:status"),
        ],
        vec![InlineKeyboardButton::callback("Back", "menu:main")],
    ])
}

fn manage_menu_markup() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback("Back", "menu:main")]])
}

fn cancel_menu_markup() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback("Cancel", "action:cancel")]])
}

async fn send_callback_menu(
    bot: &Bot,
    query: &CallbackQuery,
    text: &str,
    markup: InlineKeyboardMarkup,
) -> ResponseResult<()> {
    if let Some(message) = query.message.as_ref() {
        bot.send_message(message.chat().id, text)
            .reply_markup(markup)
            .await?;
    } else {
        bot.send_message(query.from.id, text)
            .reply_markup(markup)
            .await?;
    }

    bot.answer_callback_query(query.id.clone()).await?;
    Ok(())
}

async fn send_tracked_wallets(
    bot: &Bot,
    chat_id: ChatId,
    db: &Db,
    user_id: i64,
) -> ResponseResult<()> {
    let wallets = match db::list_tracked_wallets(db, user_id).await {
        Ok(wallets) => wallets,
        Err(_err) => {
            bot.send_message(chat_id, "Sorry, I couldn't load your tracked wallets.")
                .await?;
            return Ok(());
        }
    };

    if wallets.is_empty() {
        bot.send_message(chat_id, "No tracked wallets yet.").await?;
        return Ok(());
    }

    let mut lines = Vec::with_capacity(wallets.len());
    for wallet in wallets {
        match wallet.label {
            Some(label) => lines.push(format!("- {} ({})", wallet.wallet_address, label)),
            None => lines.push(format!("- {}", wallet.wallet_address)),
        }
    }

    bot.send_message(chat_id, format!("Tracked wallets:\n{}", lines.join("\n")))
        .await?;
    Ok(())
}

async fn send_track_status(
    bot: &Bot,
    chat_id: ChatId,
    db: &Db,
    user_id: i64,
) -> ResponseResult<()> {
    let count = match db::count_tracked_wallets(db, user_id).await {
        Ok(count) => count,
        Err(_err) => {
            bot.send_message(chat_id, "Sorry, I couldn't load tracking status.").await?;
            return Ok(());
        }
    };

    bot.send_message(
        chat_id,
        format!("Tracking {count} wallet(s). Monitoring is not enabled yet."),
    )
    .await?;
    Ok(())
}
