//! Message and callback-query handling for private chats.
//!
//! Every entry point enforces the allowlist first, then dispatches on the
//! stored pending action or the callback payload.

use std::sync::Arc;

use polymarket_client_sdk::clob::types::SignatureType;
use teloxide::payloads::SendMessageSetters;
use teloxide::prelude::*;

use crate::db::{self, Db};
use crate::state::AppState;
use crate::utils::crypto::EncryptionKey;

use super::common::{
    ACTION_COPY_TRADE_EDIT_PRICE, ACTION_COPY_TRADE_EDIT_SIZE, ACTION_MANAGE_AUTH_KEY,
    ACTION_MANAGE_AUTH_LABEL, ACTION_MANAGE_CANCEL_ORDER, ACTION_MANAGE_LIMIT_ORDER,
    ACTION_MANAGE_MARKET_ORDER, ACTION_MANAGE_POSITIONS, ACTION_TRACK_ADD_ADDRESS,
    ACTION_TRACK_ADD_LABEL, ACTION_TRACK_REMOVE, MSG_ACTION_EXPIRED, SIG_EOA, SIG_PROXY,
    callback_chat_id, log_db_error,
};

use super::copy_trade::{
    handle_copy_trade_confirm, handle_copy_trade_flip, handle_copy_trade_init,
    handle_copy_trade_toggle_type, load_owned_copy_trade_state,
};
use super::manage::{
    confirm_managed_wallet_removal, finalize_manage_label, handle_show_positions,
    prompt_managed_wallet_removal, send_manage_menu, send_managed_positions, send_managed_wallet,
    set_managed_wallet_type,
};
use super::menus::{
    HELP_TEXT, cancel_menu_markup, main_menu_markup, manage_cancel_menu_markup, manage_menu_markup,
    manage_wallet_type_change_markup, manage_wallet_type_setup_markup, send_callback_menu,
    send_track_menu, track_menu_markup,
};
use super::parse::parse_incoming_command;
use super::track::{finalize_track_add, send_tracked_wallets};

/// Handle an incoming private message: commands or pending-action input.
pub async fn handle_message(
    bot: Bot,
    msg: Message,
    state: Arc<AppState>,
    bot_name: String,
) -> ResponseResult<()> {
    let db = state.db.clone();
    let encryption_key = state.config.encryption_key.clone();
    let allowed_telegram_ids = &state.config.allowed_telegram_ids;
    if !msg.chat.is_private() {
        bot.send_message(
            msg.chat.id,
            "For security, this bot only works in private chats.",
        )
        .await?;
        return Ok(());
    }

    let text = match msg.text() {
        Some(text) => text.to_string(),
        None => return Ok(()),
    };

    let Some(user) = msg.from.as_ref() else {
        bot.send_message(msg.chat.id, "This bot only supports direct messages.")
            .await?;
        return Ok(());
    };

    if !allowed_telegram_ids.is_allowed(user.id.0 as i64) {
        bot.send_message(
            msg.chat.id,
            format!(
                "⛔ You are not authorized to use this bot.\nYour Telegram ID: {}",
                user.id.0
            ),
        )
        .await?;
        return Ok(());
    }

    let telegram_id = user.id.0 as i64;
    let chat_id = msg.chat.id.0;
    let user_id = match db::ensure_user(&db, telegram_id, chat_id).await {
        Ok(user_id) => user_id,
        Err(_err) => {
            bot.send_message(
                msg.chat.id,
                "Sorry, I couldn't update your profile. Try again soon.",
            )
            .await?;
            return Ok(());
        }
    };

    if let Some((command, _args)) = parse_incoming_command(text.as_str(), bot_name.as_str()) {
        log_db_error(
            db::clear_pending_state(&db, user_id).await,
            "clear_pending_state",
            user_id,
        );
        return handle_top_level_command(bot, msg, &db, user_id, command.as_str()).await;
    }

    match db::get_pending_state(&db, user_id).await {
        Ok((Some(action), data)) => {
            return handle_pending_action(
                bot,
                msg,
                &db,
                user_id,
                action.as_str(),
                data.as_deref(),
                text.as_str(),
                encryption_key,
            )
            .await;
        }
        Ok((None, _)) => {}
        Err(_err) => {
            bot.send_message(msg.chat.id, "Sorry, I couldn't read your session state.")
                .await?;
            return Ok(());
        }
    }

    bot.send_message(msg.chat.id, "Use /start to open the menu.")
        .await?;
    Ok(())
}

/// Handle an inline-button callback: menus, positions, or copy-trade flow.
pub async fn handle_callback(
    bot: Bot,
    query: CallbackQuery,
    state: Arc<AppState>,
) -> ResponseResult<()> {
    let db = state.db.clone();
    let encryption_key = state.config.encryption_key.clone();
    let allowed_telegram_ids = &state.config.allowed_telegram_ids;
    if let Some(message) = query.message.as_ref() {
        if !message.chat().is_private() {
            bot.answer_callback_query(query.id).await?;
            return Ok(());
        }
    }

    if !allowed_telegram_ids.is_allowed(query.from.id.0 as i64) {
        bot.answer_callback_query(query.id)
            .text(format!(
                "⛔ Not authorized. Your Telegram ID: {}",
                query.from.id.0
            ))
            .show_alert(true)
            .await?;
        return Ok(());
    }

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
            log_db_error(
                db::clear_pending_state(&db, user_id).await,
                "clear_pending_state",
                user_id,
            );
            send_callback_menu(&bot, &query, "What do you want to do?", main_menu_markup()).await?;
        }
        "menu:track" => {
            log_db_error(
                db::clear_pending_state(&db, user_id).await,
                "clear_pending_state",
                user_id,
            );
            log_db_error(
                db::set_mode(&db, user_id, "track").await,
                "set_mode",
                user_id,
            );
            send_callback_menu(
                &bot,
                &query,
                "Track wallets: add, remove, or review your list.",
                track_menu_markup(),
            )
            .await?;
        }
        "menu:manage" => {
            log_db_error(
                db::clear_pending_state(&db, user_id).await,
                "clear_pending_state",
                user_id,
            );
            log_db_error(
                db::set_mode(&db, user_id, "manage").await,
                "set_mode",
                user_id,
            );
            send_callback_menu(
                &bot,
                &query,
                "Manage your trading wallet, orders, and positions.",
                manage_menu_markup(),
            )
            .await?;
        }
        "manage:auth" => {
            log_db_error(
                db::clear_pending_state(&db, user_id).await,
                "clear_pending_state",
                user_id,
            );
            send_callback_menu(
                &bot,
                &query,
                "Choose how this wallet should sign (Magic vs Standard).",
                manage_wallet_type_setup_markup(),
            )
            .await?;
        }
        "manage:auth_type_eoa" => {
            let _ =
                db::set_pending_state(&db, user_id, Some(ACTION_MANAGE_AUTH_KEY), Some(SIG_EOA))
                    .await;
            send_callback_menu(
                &bot,
                &query,
                "Send the private key for this wallet.\n\n\
                ⚠️ Security: Your message will be deleted immediately, but consider using a dedicated wallet with limited funds.",
                manage_cancel_menu_markup(),
            )
            .await?;
        }
        "manage:auth_type_proxy" => {
            let _ =
                db::set_pending_state(&db, user_id, Some(ACTION_MANAGE_AUTH_KEY), Some(SIG_PROXY))
                    .await;
            send_callback_menu(
                &bot,
                &query,
                "Send the private key for this wallet.\n\n\
                ⚠️ Security: Your message will be deleted immediately, but consider using a dedicated wallet with limited funds.",
                manage_cancel_menu_markup(),
            )
            .await?;
        }
        "manage:list" => {
            log_db_error(
                db::clear_pending_state(&db, user_id).await,
                "clear_pending_state",
                user_id,
            );
            let chat_id = callback_chat_id(&query);
            send_managed_wallet(&bot, chat_id, &db, user_id).await?;
            bot.answer_callback_query(query.id).await?;
        }
        "manage:positions" => {
            log_db_error(
                db::clear_pending_state(&db, user_id).await,
                "clear_pending_state",
                user_id,
            );
            let chat_id = callback_chat_id(&query);
            send_managed_positions(&bot, chat_id, &db, user_id).await?;
            bot.answer_callback_query(query.id).await?;
        }
        "manage:market_order" => {
            let _ =
                db::set_pending_state(&db, user_id, Some(ACTION_MANAGE_MARKET_ORDER), None).await;
            send_callback_menu(
                &bot,
                &query,
                "Market order format: <token_id> <buy|sell> <amount> (buy uses USDC, sell uses shares).",
                manage_cancel_menu_markup(),
            )
            .await?;
        }
        "manage:limit_order" => {
            let _ =
                db::set_pending_state(&db, user_id, Some(ACTION_MANAGE_LIMIT_ORDER), None).await;
            send_callback_menu(
                &bot,
                &query,
                "Limit order format: <token_id> <buy|sell> <price> <size>.",
                manage_cancel_menu_markup(),
            )
            .await?;
        }
        "manage:cancel_order" => {
            let _ =
                db::set_pending_state(&db, user_id, Some(ACTION_MANAGE_CANCEL_ORDER), None).await;
            send_callback_menu(
                &bot,
                &query,
                "Cancel format: <order_id>.",
                manage_cancel_menu_markup(),
            )
            .await?;
        }
        "manage:remove" => {
            log_db_error(
                db::clear_pending_state(&db, user_id).await,
                "clear_pending_state",
                user_id,
            );
            let chat_id = callback_chat_id(&query);
            prompt_managed_wallet_removal(&bot, chat_id, &db, user_id).await?;
            bot.answer_callback_query(query.id).await?;
        }
        "manage:remove_confirm" => {
            log_db_error(
                db::clear_pending_state(&db, user_id).await,
                "clear_pending_state",
                user_id,
            );
            let chat_id = callback_chat_id(&query);
            confirm_managed_wallet_removal(&bot, chat_id, &db, user_id).await?;
            bot.answer_callback_query(query.id).await?;
        }
        "manage:wallet_type" => {
            log_db_error(
                db::clear_pending_state(&db, user_id).await,
                "clear_pending_state",
                user_id,
            );
            send_callback_menu(
                &bot,
                &query,
                "Choose how this wallet should sign (Magic vs Standard).",
                manage_wallet_type_change_markup(),
            )
            .await?;
        }
        "manage:change_type_eoa" => {
            log_db_error(
                db::clear_pending_state(&db, user_id).await,
                "clear_pending_state",
                user_id,
            );
            let chat_id = callback_chat_id(&query);
            set_managed_wallet_type(&bot, chat_id, &db, user_id, SignatureType::Eoa).await?;
            bot.answer_callback_query(query.id).await?;
        }
        "manage:change_type_proxy" => {
            log_db_error(
                db::clear_pending_state(&db, user_id).await,
                "clear_pending_state",
                user_id,
            );
            let chat_id = callback_chat_id(&query);
            set_managed_wallet_type(&bot, chat_id, &db, user_id, SignatureType::Proxy).await?;
            bot.answer_callback_query(query.id).await?;
        }
        "track:add" => {
            log_db_error(
                db::set_pending_state(&db, user_id, Some(ACTION_TRACK_ADD_ADDRESS), None).await,
                "set_pending_state",
                user_id,
            );
            send_callback_menu(
                &bot,
                &query,
                "Send the wallet address you want to track (0x...).",
                cancel_menu_markup(),
            )
            .await?;
        }
        "track:remove" => {
            log_db_error(
                db::set_pending_state(&db, user_id, Some(ACTION_TRACK_REMOVE), None).await,
                "set_pending_state",
                user_id,
            );
            send_callback_menu(
                &bot,
                &query,
                "Send the tracked address you want to remove.",
                cancel_menu_markup(),
            )
            .await?;
        }
        "track:list" => {
            log_db_error(
                db::clear_pending_state(&db, user_id).await,
                "clear_pending_state",
                user_id,
            );
            let chat_id = callback_chat_id(&query);
            send_tracked_wallets(&bot, chat_id, &db, user_id).await?;
            bot.answer_callback_query(query.id).await?;
        }
        "track:skip_label" => {
            let chat_id = callback_chat_id(&query);
            match db::get_pending_state(&db, user_id).await {
                Ok((Some(action), data)) if action == ACTION_TRACK_ADD_LABEL => {
                    if let Some(wallet_address) = data {
                        finalize_track_add(&bot, chat_id, &db, user_id, &wallet_address, None)
                            .await?;
                    } else {
                        log_db_error(
                            db::clear_pending_state(&db, user_id).await,
                            "clear_pending_state",
                            user_id,
                        );
                        send_track_menu(&bot, chat_id).await?;
                    }
                }
                Ok(_) => {
                    log_db_error(
                        db::clear_pending_state(&db, user_id).await,
                        "clear_pending_state",
                        user_id,
                    );
                    send_track_menu(&bot, chat_id).await?;
                }
                Err(_err) => {
                    bot.send_message(chat_id, "Sorry, I couldn't update that request.")
                        .await?;
                }
            }
            bot.answer_callback_query(query.id).await?;
        }
        "manage:skip_label" => {
            let chat_id = callback_chat_id(&query);
            match db::get_pending_state(&db, user_id).await {
                Ok((Some(action), data)) if action == ACTION_MANAGE_AUTH_LABEL => {
                    if let Some(wallet_address) = data {
                        finalize_manage_label(&bot, chat_id, &db, user_id, &wallet_address, None)
                            .await?;
                    } else {
                        log_db_error(
                            db::clear_pending_state(&db, user_id).await,
                            "clear_pending_state",
                            user_id,
                        );
                        send_manage_menu(&bot, chat_id).await?;
                    }
                }
                Ok(_) => {
                    log_db_error(
                        db::clear_pending_state(&db, user_id).await,
                        "clear_pending_state",
                        user_id,
                    );
                    send_manage_menu(&bot, chat_id).await?;
                }
                Err(_err) => {
                    bot.send_message(chat_id, "Sorry, I couldn't update that request.")
                        .await?;
                }
            }
            bot.answer_callback_query(query.id).await?;
        }
        "action:cancel" => {
            log_db_error(
                db::clear_pending_state(&db, user_id).await,
                "clear_pending_state",
                user_id,
            );
            send_callback_menu(
                &bot,
                &query,
                "Track wallets: add, remove, or review your list.",
                track_menu_markup(),
            )
            .await?;
        }
        "manage:cancel_action" => {
            log_db_error(
                db::clear_pending_state(&db, user_id).await,
                "clear_pending_state",
                user_id,
            );
            send_callback_menu(
                &bot,
                &query,
                "Manage your trading wallet, orders, and positions.",
                manage_menu_markup(),
            )
            .await?;
        }
        data if data.starts_with("sp:") => {
            let chat_id = callback_chat_id(&query);
            if let Some(id_str) = data.strip_prefix("sp:") {
                bot.answer_callback_query(query.id).await?;
                if let Ok(cb_id) = id_str.parse::<i64>() {
                    handle_show_positions(
                        &bot,
                        chat_id,
                        &db,
                        user_id,
                        cb_id,
                        query.message.as_ref(),
                    )
                    .await?;
                }
            }
        }
        data if data.starts_with("ct:") => {
            let chat_id = callback_chat_id(&query);
            if let Some(id_str) = data.strip_prefix("ct:") {
                if let Ok(cb_id) = id_str.parse::<i64>() {
                    handle_copy_trade_init(&bot, chat_id, &db, user_id, cb_id).await?;
                }
            }
            bot.answer_callback_query(query.id).await?;
        }
        data if data.starts_with("ct_confirm:") => {
            let chat_id = callback_chat_id(&query);
            if let Some(id_str) = data.strip_prefix("ct_confirm:") {
                if let Ok(ct_id) = id_str.parse::<i64>() {
                    handle_copy_trade_confirm(
                        &bot,
                        chat_id,
                        &db,
                        user_id,
                        ct_id,
                        encryption_key.clone(),
                    )
                    .await?;
                }
            }
            bot.answer_callback_query(query.id).await?;
        }
        data if data.starts_with("ct_cancel:") => {
            let chat_id = callback_chat_id(&query);
            if let Some(id_str) = data.strip_prefix("ct_cancel:") {
                if let Ok(ct_id) = id_str.parse::<i64>() {
                    if load_owned_copy_trade_state(&bot, chat_id, &db, user_id, ct_id)
                        .await
                        .is_some()
                    {
                        log_db_error(
                            db::delete_copy_trade_state(&db, ct_id).await,
                            "delete_copy_trade_state",
                            user_id,
                        );
                        log_db_error(
                            db::clear_pending_state(&db, user_id).await,
                            "clear_pending_state",
                            user_id,
                        );
                        bot.send_message(chat_id, "Copy trade cancelled.").await?;
                    }
                }
            }
            bot.answer_callback_query(query.id).await?;
        }
        data if data.starts_with("ct_flip:") => {
            let chat_id = callback_chat_id(&query);
            if let Some(id_str) = data.strip_prefix("ct_flip:") {
                if let Ok(ct_id) = id_str.parse::<i64>() {
                    handle_copy_trade_flip(&bot, chat_id, &db, user_id, ct_id, &query).await?;
                }
            }
            bot.answer_callback_query(query.id).await?;
        }
        data if data.starts_with("ct_market:") => {
            let chat_id = callback_chat_id(&query);
            if let Some(id_str) = data.strip_prefix("ct_market:") {
                if let Ok(ct_id) = id_str.parse::<i64>() {
                    handle_copy_trade_toggle_type(&bot, chat_id, &db, user_id, ct_id, &query)
                        .await?;
                }
            }
            bot.answer_callback_query(query.id).await?;
        }
        data if data.starts_with("ct_price:") => {
            if let Some(id_str) = data.strip_prefix("ct_price:") {
                if let Ok(ct_id) = id_str.parse::<i64>() {
                    log_db_error(
                        db::set_pending_state(
                            &db,
                            user_id,
                            Some(ACTION_COPY_TRADE_EDIT_PRICE),
                            Some(&ct_id.to_string()),
                        )
                        .await,
                        "set_pending_state",
                        user_id,
                    );
                    let chat_id = callback_chat_id(&query);
                    bot.send_message(chat_id, "Send the new price (e.g., 0.47):")
                        .await?;
                }
            }
            bot.answer_callback_query(query.id).await?;
        }
        data if data.starts_with("ct_size:") => {
            if let Some(id_str) = data.strip_prefix("ct_size:") {
                if let Ok(ct_id) = id_str.parse::<i64>() {
                    log_db_error(
                        db::set_pending_state(
                            &db,
                            user_id,
                            Some(ACTION_COPY_TRADE_EDIT_SIZE),
                            Some(&ct_id.to_string()),
                        )
                        .await,
                        "set_pending_state",
                        user_id,
                    );
                    let chat_id = callback_chat_id(&query);
                    bot.send_message(chat_id, "Send the new size (number of shares):")
                        .await?;
                }
            }
            bot.answer_callback_query(query.id).await?;
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
        "version" => {
            bot.send_message(msg.chat.id, format!("albedo {}", crate::VERSION))
                .await?;
        }
        "track" => {
            log_db_error(
                db::set_mode(db, user_id, "track").await,
                "set_mode",
                user_id,
            );
            send_track_menu(&bot, msg.chat.id).await?;
        }
        "manage" => {
            log_db_error(
                db::set_mode(db, user_id, "manage").await,
                "set_mode",
                user_id,
            );
            bot.send_message(
                msg.chat.id,
                "Manage your trading wallet, orders, and positions.",
            )
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
    bot.send_message(msg.chat.id, "What do you want to do?")
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
    data: Option<&str>,
    input: &str,
    encryption_key: Option<EncryptionKey>,
) -> ResponseResult<()> {
    match action {
        ACTION_TRACK_ADD_ADDRESS => {
            super::track::handle_address_input(&bot, &msg, db, user_id, input).await?;
        }
        ACTION_TRACK_ADD_LABEL => {
            super::track::handle_label_input(&bot, &msg, db, user_id, data, input).await?;
        }
        ACTION_TRACK_REMOVE => {
            super::track::handle_remove_input(&bot, &msg, db, user_id, input).await?;
        }
        ACTION_MANAGE_AUTH_KEY => {
            super::manage::handle_auth_key_input(
                &bot,
                &msg,
                db,
                user_id,
                data,
                input,
                encryption_key,
            )
            .await?
        }
        ACTION_MANAGE_AUTH_LABEL => {
            super::manage::handle_auth_label_input(&bot, &msg, db, user_id, data, input).await?
        }
        ACTION_MANAGE_POSITIONS => {
            super::manage::handle_positions_input(&bot, &msg, db, user_id).await?
        }
        ACTION_MANAGE_MARKET_ORDER => {
            super::orders::handle_market_order_input(&bot, &msg, db, user_id, input, encryption_key)
                .await?
        }
        ACTION_MANAGE_LIMIT_ORDER => {
            super::orders::handle_limit_order_input(&bot, &msg, db, user_id, input, encryption_key)
                .await?
        }
        ACTION_MANAGE_CANCEL_ORDER => {
            super::orders::handle_cancel_order_input(&bot, &msg, db, user_id, input, encryption_key)
                .await?
        }
        ACTION_COPY_TRADE_EDIT_PRICE => {
            super::copy_trade::handle_price_input(&bot, &msg, db, user_id, data, input).await?
        }
        ACTION_COPY_TRADE_EDIT_SIZE => {
            super::copy_trade::handle_size_input(&bot, &msg, db, user_id, data, input).await?
        }
        _ => {
            log_db_error(
                db::clear_pending_state(db, user_id).await,
                "clear_pending_state",
                user_id,
            );
            bot.send_message(msg.chat.id, MSG_ACTION_EXPIRED).await?;
        }
    }

    Ok(())
}
