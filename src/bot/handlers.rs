//! Message and callback-query handling for private chats.
//!
//! Every entry point enforces the allowlist first, then dispatches on the
//! stored pending action or the callback payload.

use std::sync::Arc;

use polymarket_client_sdk::clob::types::SignatureType;
use polymarket_client_sdk::data::Client as DataClient;
use teloxide::payloads::SendMessageSetters;
use teloxide::prelude::*;

use crate::db::{self, Db, WalletMode};
use crate::state::AppState;
use crate::utils::crypto::EncryptionKey;

use super::common::{
    ACTION_ARCHIVE_ADD_ADDRESS, ACTION_ARCHIVE_ADD_LABEL, ACTION_ARCHIVE_REMOVE,
    ACTION_COPY_TRADE_EDIT_PRICE, ACTION_COPY_TRADE_EDIT_SIZE, ACTION_MANAGE_AUTH_KEY,
    ACTION_MANAGE_AUTH_LABEL, ACTION_MANAGE_CANCEL_ORDER, ACTION_MANAGE_LIMIT_ORDER,
    ACTION_MANAGE_MARKET_ORDER, ACTION_MANAGE_POSITIONS, ACTION_TRACK_ADD_ADDRESS,
    ACTION_TRACK_ADD_LABEL, ACTION_TRACK_REMOVE, MSG_ACTION_EXPIRED, SIG_EOA, SIG_PROXY,
    callback_chat_id, log_db_error, telegram_user_id,
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
    ARCHIVE_MENU_TEXT, HELP_TEXT, archive_menu_markup, main_menu_markup, manage_cancel_menu_markup,
    manage_menu_markup, manage_wallet_type_change_markup, manage_wallet_type_setup_markup,
    send_callback_menu, send_track_menu, send_wallet_menu, track_menu_markup, wallet_cancel_markup,
};
use super::parse::parse_incoming_command;
use super::track::{finalize_wallet_add, send_wallets};

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

    if !allowed_telegram_ids.is_allowed(telegram_user_id(user.id.0)) {
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

    let telegram_id = telegram_user_id(user.id.0);
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
        return Box::pin(handle_top_level_command(
            bot,
            msg,
            &db,
            user_id,
            command.as_str(),
        ))
        .await;
    }

    match db::get_pending_state(&db, user_id).await {
        Ok((Some(action), data)) => {
            return handle_pending_action(
                bot,
                &state.data_client,
                msg,
                &db,
                user_id,
                PendingAction {
                    action: action.as_str(),
                    data: data.as_deref(),
                    input: text.as_str(),
                },
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
// A flat dispatch over every action key. The arms are independent, so
// splitting them into helpers would scatter one decision table.
#[expect(
    clippy::too_many_lines,
    reason = "single dispatch table over action keys"
)]
pub async fn handle_callback(
    bot: Bot,
    query: CallbackQuery,
    state: Arc<AppState>,
) -> ResponseResult<()> {
    let db = state.db.clone();
    let encryption_key = state.config.encryption_key.clone();
    let allowed_telegram_ids = &state.config.allowed_telegram_ids;
    if let Some(message) = query.message.as_ref()
        && !message.chat().is_private()
    {
        bot.answer_callback_query(query.id).await?;
        return Ok(());
    }

    if !allowed_telegram_ids.is_allowed(telegram_user_id(query.from.id.0)) {
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
        .map_or(telegram_user_id(query.from.id.0), |message| {
            message.chat().id.0
        });

    let user_id = match db::ensure_user(&db, telegram_user_id(query.from.id.0), chat_id).await {
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
                db::set_mode(&db, user_id, db::UserMode::Track).await,
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
        "menu:archive" | "archive:cancel" => {
            log_db_error(
                db::clear_pending_state(&db, user_id).await,
                "clear_pending_state",
                user_id,
            );
            send_callback_menu(&bot, &query, ARCHIVE_MENU_TEXT, archive_menu_markup()).await?;
        }
        "menu:manage" => {
            log_db_error(
                db::clear_pending_state(&db, user_id).await,
                "clear_pending_state",
                user_id,
            );
            log_db_error(
                db::set_mode(&db, user_id, db::UserMode::Manage).await,
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
            send_managed_positions(&bot, &state.data_client, chat_id, &db, user_id).await?;
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
        "track:add" | "archive:add" => {
            let mode = if data == "archive:add" {
                WalletMode::Archived
            } else {
                WalletMode::Tracked
            };
            let action = if mode.is_archived() {
                ACTION_ARCHIVE_ADD_ADDRESS
            } else {
                ACTION_TRACK_ADD_ADDRESS
            };
            log_db_error(
                db::set_pending_state(&db, user_id, Some(action), None).await,
                "set_pending_state",
                user_id,
            );
            send_callback_menu(
                &bot,
                &query,
                if mode.is_archived() { "Send the wallet address to archive (0x...). If tracked, it will stop being monitored. You can add a label next." } else { "Send the wallet address you want to track (0x...). Archived wallets will resume monitoring." },
                wallet_cancel_markup(mode),
            )
            .await?;
        }
        "track:remove" | "archive:remove" => {
            let mode = if data == "archive:remove" {
                WalletMode::Archived
            } else {
                WalletMode::Tracked
            };
            let action = if mode.is_archived() {
                ACTION_ARCHIVE_REMOVE
            } else {
                ACTION_TRACK_REMOVE
            };
            log_db_error(
                db::set_pending_state(&db, user_id, Some(action), None).await,
                "set_pending_state",
                user_id,
            );
            send_callback_menu(
                &bot,
                &query,
                if mode.is_archived() { "Send the archived address you want to delete from your saved list." } else { "Send the tracked address you want to remove. To keep it saved, use View all / Archive wallets instead." },
                wallet_cancel_markup(mode),
            )
            .await?;
        }
        "track:list" | "archive:list" => {
            let mode = if data == "archive:list" {
                WalletMode::Archived
            } else {
                WalletMode::Tracked
            };
            log_db_error(
                db::clear_pending_state(&db, user_id).await,
                "clear_pending_state",
                user_id,
            );
            bot.answer_callback_query(query.id.clone()).await?;
            let chat_id = callback_chat_id(&query);
            send_wallets(&bot, chat_id, &db, user_id, mode).await?;
        }
        "track:skip_label" | "archive:skip_label" => {
            let mode = if data == "archive:skip_label" {
                WalletMode::Archived
            } else {
                WalletMode::Tracked
            };
            let expected_action = if mode.is_archived() {
                ACTION_ARCHIVE_ADD_LABEL
            } else {
                ACTION_TRACK_ADD_LABEL
            };
            let chat_id = callback_chat_id(&query);
            match db::get_pending_state(&db, user_id).await {
                Ok((Some(action), data)) if action == expected_action => {
                    if let Some(wallet_address) = data {
                        finalize_wallet_add(
                            &bot,
                            chat_id,
                            &db,
                            user_id,
                            &wallet_address,
                            None,
                            mode,
                        )
                        .await?;
                    } else {
                        log_db_error(
                            db::clear_pending_state(&db, user_id).await,
                            "clear_pending_state",
                            user_id,
                        );
                        send_wallet_menu(&bot, chat_id, mode).await?;
                    }
                }
                Ok(_) => {
                    bot.send_message(chat_id, MSG_ACTION_EXPIRED).await?;
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
        data if data.starts_with("wallet:archive:") || data.starts_with("wallet:track:") => {
            bot.answer_callback_query(query.id.clone()).await?;
            let mode = if data.starts_with("wallet:archive:") {
                WalletMode::Archived
            } else {
                WalletMode::Tracked
            };
            let chat_id = callback_chat_id(&query);
            if let Some(id) = data
                .rsplit(':')
                .next()
                .and_then(|id| id.parse::<i64>().ok())
            {
                match db::move_wallet(&db, user_id, id, mode).await {
                    Ok(true) => {
                        log_db_error(
                            db::clear_pending_state(&db, user_id).await,
                            "clear_pending_state",
                            user_id,
                        );
                        bot.send_message(chat_id, if mode.is_archived() { "Wallet archived. Its address and label are saved. Monitoring stopped." } else { "Wallet is now tracked. Monitoring resumes from its next poll." }).await?;
                        send_wallet_menu(&bot, chat_id, mode).await?;
                    }
                    Ok(false) => {
                        bot.send_message(
                            chat_id,
                            "That wallet has already moved or was removed. Open the list again.",
                        )
                        .await?;
                    }
                    Err(_) => {
                        bot.send_message(
                            chat_id,
                            "Sorry, I couldn't move that wallet. Try again soon.",
                        )
                        .await?;
                    }
                }
            }
        }
        data if data.starts_with("sp:") => {
            let chat_id = callback_chat_id(&query);
            if let Some(id_str) = data.strip_prefix("sp:") {
                bot.answer_callback_query(query.id).await?;
                if let Ok(cb_id) = id_str.parse::<i64>() {
                    handle_show_positions(
                        &bot,
                        &state.data_client,
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
            if let Some(id_str) = data.strip_prefix("ct:")
                && let Ok(cb_id) = id_str.parse::<i64>()
            {
                handle_copy_trade_init(&bot, chat_id, &db, user_id, cb_id).await?;
            }
            bot.answer_callback_query(query.id).await?;
        }
        data if data.starts_with("ct_confirm:") => {
            let chat_id = callback_chat_id(&query);
            if let Some(id_str) = data.strip_prefix("ct_confirm:")
                && let Ok(ct_id) = id_str.parse::<i64>()
            {
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
            bot.answer_callback_query(query.id).await?;
        }
        data if data.starts_with("ct_cancel:") => {
            let chat_id = callback_chat_id(&query);
            if let Some(id_str) = data.strip_prefix("ct_cancel:")
                && let Ok(ct_id) = id_str.parse::<i64>()
                && load_owned_copy_trade_state(&bot, chat_id, &db, user_id, ct_id)
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
            bot.answer_callback_query(query.id).await?;
        }
        data if data.starts_with("ct_flip:") => {
            let chat_id = callback_chat_id(&query);
            if let Some(id_str) = data.strip_prefix("ct_flip:")
                && let Ok(ct_id) = id_str.parse::<i64>()
            {
                handle_copy_trade_flip(&bot, chat_id, &db, user_id, ct_id, &query).await?;
            }
            bot.answer_callback_query(query.id).await?;
        }
        data if data.starts_with("ct_market:") => {
            let chat_id = callback_chat_id(&query);
            if let Some(id_str) = data.strip_prefix("ct_market:")
                && let Ok(ct_id) = id_str.parse::<i64>()
            {
                handle_copy_trade_toggle_type(&bot, chat_id, &db, user_id, ct_id, &query).await?;
            }
            bot.answer_callback_query(query.id).await?;
        }
        data if data.starts_with("ct_price:") => {
            if let Some(id_str) = data.strip_prefix("ct_price:")
                && let Ok(ct_id) = id_str.parse::<i64>()
            {
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
            bot.answer_callback_query(query.id).await?;
        }
        data if data.starts_with("ct_size:") => {
            if let Some(id_str) = data.strip_prefix("ct_size:")
                && let Ok(ct_id) = id_str.parse::<i64>()
            {
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
                db::set_mode(db, user_id, db::UserMode::Track).await,
                "set_mode",
                user_id,
            );
            send_track_menu(&bot, msg.chat.id).await?;
        }
        "archive" => {
            send_wallet_menu(&bot, msg.chat.id, WalletMode::Archived).await?;
        }
        "manage" => {
            log_db_error(
                db::set_mode(db, user_id, db::UserMode::Manage).await,
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

/// A stored pending action plus the message the user sent in reply to it.
struct PendingAction<'a> {
    action: &'a str,
    data: Option<&'a str>,
    input: &'a str,
}

async fn handle_pending_action(
    bot: Bot,
    client: &DataClient,
    msg: Message,
    db: &Db,
    user_id: i64,
    pending: PendingAction<'_>,
    encryption_key: Option<EncryptionKey>,
) -> ResponseResult<()> {
    let PendingAction {
        action,
        data,
        input,
    } = pending;
    match action {
        ACTION_TRACK_ADD_ADDRESS | ACTION_ARCHIVE_ADD_ADDRESS => {
            let mode = if action == ACTION_ARCHIVE_ADD_ADDRESS {
                WalletMode::Archived
            } else {
                WalletMode::Tracked
            };
            super::track::handle_address_input(&bot, &msg, db, user_id, input, mode).await?;
        }
        ACTION_TRACK_ADD_LABEL | ACTION_ARCHIVE_ADD_LABEL => {
            let mode = if action == ACTION_ARCHIVE_ADD_LABEL {
                WalletMode::Archived
            } else {
                WalletMode::Tracked
            };
            super::track::handle_label_input(&bot, &msg, db, user_id, data, input, mode).await?;
        }
        ACTION_TRACK_REMOVE | ACTION_ARCHIVE_REMOVE => {
            let mode = if action == ACTION_ARCHIVE_REMOVE {
                WalletMode::Archived
            } else {
                WalletMode::Tracked
            };
            super::track::handle_remove_input(&bot, &msg, db, user_id, input, mode).await?;
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
            .await?;
        }
        ACTION_MANAGE_AUTH_LABEL => {
            super::manage::handle_auth_label_input(&bot, &msg, db, user_id, data, input).await?;
        }
        ACTION_MANAGE_POSITIONS => {
            super::manage::handle_positions_input(&bot, client, &msg, db, user_id).await?;
        }
        ACTION_MANAGE_MARKET_ORDER => {
            super::orders::handle_market_order_input(
                &bot,
                &msg,
                db,
                user_id,
                input,
                encryption_key,
            )
            .await?;
        }
        ACTION_MANAGE_LIMIT_ORDER => {
            super::orders::handle_limit_order_input(&bot, &msg, db, user_id, input, encryption_key)
                .await?;
        }
        ACTION_MANAGE_CANCEL_ORDER => {
            super::orders::handle_cancel_order_input(
                &bot,
                &msg,
                db,
                user_id,
                input,
                encryption_key,
            )
            .await?;
        }
        ACTION_COPY_TRADE_EDIT_PRICE => {
            super::copy_trade::handle_price_input(&bot, &msg, db, user_id, data, input).await?;
        }
        ACTION_COPY_TRADE_EDIT_SIZE => {
            super::copy_trade::handle_size_input(&bot, &msg, db, user_id, data, input).await?;
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
