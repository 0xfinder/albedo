use std::str::FromStr;

use polymarket_client_sdk::auth::{LocalSigner, Signer};
use polymarket_client_sdk::clob::types::{Amount, Side, SignatureType};
use polymarket_client_sdk::clob::Client as ClobClient;
use polymarket_client_sdk::data::types::request::PositionsRequest;
use polymarket_client_sdk::data::Client as DataClient;
use polymarket_client_sdk::types::{Address, Decimal, U256};
use polymarket_client_sdk::{derive_proxy_wallet, POLYGON};
use teloxide::payloads::SendMessageSetters;
use teloxide::prelude::*;
use teloxide::types::{CallbackQuery, InlineKeyboardButton, InlineKeyboardMarkup, ParseMode};
use teloxide::utils::command::parse_command;

use crate::db::{self, Db};
use crate::utils::crypto::{self, EncryptionKey};

const HELP_TEXT: &str = "Available commands:\n\
/start - Start the bot\n\
/help - Show this help message\n\
/track - Open the track menu\n\
/manage - Open the manage menu";

const ACTION_TRACK_ADD_ADDRESS: &str = "track_add_address";
const ACTION_TRACK_ADD_LABEL: &str = "track_add_label";
const ACTION_TRACK_REMOVE: &str = "track_remove";
const ACTION_MANAGE_AUTH_KEY: &str = "manage_auth_key";
const ACTION_MANAGE_AUTH_LABEL: &str = "manage_auth_label";
const ACTION_MANAGE_POSITIONS: &str = "manage_positions";
const ACTION_MANAGE_MARKET_ORDER: &str = "manage_market_order";
const ACTION_MANAGE_LIMIT_ORDER: &str = "manage_limit_order";
const ACTION_MANAGE_CANCEL_ORDER: &str = "manage_cancel_order";

pub async fn handle_message(
    bot: Bot,
    msg: Message,
    db: Db,
    bot_name: String,
    encryption_key: Option<EncryptionKey>,
) -> ResponseResult<()> {
    if !msg.chat.is_private() {
        bot.send_message(msg.chat.id, "For security, this bot only works in private chats.")
            .await?;
        return Ok(());
    }

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
        let _ = db::clear_pending_state(&db, user_id).await;
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
            bot.send_message(msg.chat.id, "Sorry, I couldn't read your session state.").await?;
            return Ok(());
        }
    }

    bot.send_message(msg.chat.id, "Use /start to open the menu.").await?;
    Ok(())
}

pub async fn handle_callback(
    bot: Bot,
    query: CallbackQuery,
    db: Db,
    _encryption_key: Option<EncryptionKey>,
) -> ResponseResult<()> {
    if let Some(message) = query.message.as_ref() {
        if !message.chat().is_private() {
            bot.answer_callback_query(query.id).await?;
            return Ok(());
        }
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
            let _ = db::clear_pending_state(&db, user_id).await;
            send_callback_menu(&bot, &query, "What do you want to do?", main_menu_markup()).await?;
        }
        "menu:track" => {
            let _ = db::clear_pending_state(&db, user_id).await;
            let _ = db::set_mode(&db, user_id, "track").await;
            send_callback_menu(
                &bot,
                &query,
                "Track wallets: add, remove, or review your list.",
                track_menu_markup(),
            )
            .await?;
        }
        "menu:manage" => {
            let _ = db::clear_pending_state(&db, user_id).await;
            let _ = db::set_mode(&db, user_id, "manage").await;
            send_callback_menu(
                &bot,
                &query,
                "Manage your trading wallet, orders, and positions.",
                manage_menu_markup(),
            )
            .await?;
        }
        "manage:auth" => {
            let _ = db::clear_pending_state(&db, user_id).await;
            send_callback_menu(
                &bot,
                &query,
                "Choose how this wallet should sign (Magic vs Standard).",
                manage_wallet_type_setup_markup(),
            )
            .await?;
        }
        "manage:auth_type_eoa" => {
            let _ = db::set_pending_state(&db, user_id, Some(ACTION_MANAGE_AUTH_KEY), Some("sig:0"))
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
            let _ = db::set_pending_state(&db, user_id, Some(ACTION_MANAGE_AUTH_KEY), Some("sig:1"))
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
            let _ = db::clear_pending_state(&db, user_id).await;
            let chat_id = query
                .message
                .as_ref()
                .map(|message| message.chat().id)
                .unwrap_or(ChatId(query.from.id.0 as i64));
            send_managed_wallet(&bot, chat_id, &db, user_id).await?;
            bot.answer_callback_query(query.id).await?;
        }
        "manage:positions" => {
            let _ = db::clear_pending_state(&db, user_id).await;
            let chat_id = query
                .message
                .as_ref()
                .map(|message| message.chat().id)
                .unwrap_or(ChatId(query.from.id.0 as i64));
            send_managed_positions(&bot, chat_id, &db, user_id).await?;
            bot.answer_callback_query(query.id).await?;
        }
        "manage:market_order" => {
            let _ = db::set_pending_state(&db, user_id, Some(ACTION_MANAGE_MARKET_ORDER), None)
                .await;
            send_callback_menu(
                &bot,
                &query,
                "Market order format: <token_id> <buy|sell> <amount> (buy uses USDC, sell uses shares).",
                manage_cancel_menu_markup(),
            )
            .await?;
        }
        "manage:limit_order" => {
            let _ = db::set_pending_state(&db, user_id, Some(ACTION_MANAGE_LIMIT_ORDER), None)
                .await;
            send_callback_menu(
                &bot,
                &query,
                "Limit order format: <token_id> <buy|sell> <price> <size>.",
                manage_cancel_menu_markup(),
            )
            .await?;
        }
        "manage:cancel_order" => {
            let _ = db::set_pending_state(&db, user_id, Some(ACTION_MANAGE_CANCEL_ORDER), None)
                .await;
            send_callback_menu(
                &bot,
                &query,
                "Cancel format: <order_id>.",
                manage_cancel_menu_markup(),
            )
            .await?;
        }
        "manage:remove" => {
            let _ = db::clear_pending_state(&db, user_id).await;
            let chat_id = query
                .message
                .as_ref()
                .map(|message| message.chat().id)
                .unwrap_or(ChatId(query.from.id.0 as i64));
            prompt_managed_wallet_removal(&bot, chat_id, &db, user_id).await?;
            bot.answer_callback_query(query.id).await?;
        }
        "manage:remove_confirm" => {
            let _ = db::clear_pending_state(&db, user_id).await;
            let chat_id = query
                .message
                .as_ref()
                .map(|message| message.chat().id)
                .unwrap_or(ChatId(query.from.id.0 as i64));
            confirm_managed_wallet_removal(&bot, chat_id, &db, user_id).await?;
            bot.answer_callback_query(query.id).await?;
        }
        "manage:wallet_type" => {
            let _ = db::clear_pending_state(&db, user_id).await;
            send_callback_menu(
                &bot,
                &query,
                "Choose how this wallet should sign (Magic vs Standard).",
                manage_wallet_type_change_markup(),
            )
            .await?;
        }
        "manage:change_type_eoa" => {
            let _ = db::clear_pending_state(&db, user_id).await;
            let chat_id = query
                .message
                .as_ref()
                .map(|message| message.chat().id)
                .unwrap_or(ChatId(query.from.id.0 as i64));
            set_managed_wallet_type(&bot, chat_id, &db, user_id, SignatureType::Eoa).await?;
            bot.answer_callback_query(query.id).await?;
        }
        "manage:change_type_proxy" => {
            let _ = db::clear_pending_state(&db, user_id).await;
            let chat_id = query
                .message
                .as_ref()
                .map(|message| message.chat().id)
                .unwrap_or(ChatId(query.from.id.0 as i64));
            set_managed_wallet_type(&bot, chat_id, &db, user_id, SignatureType::Proxy).await?;
            bot.answer_callback_query(query.id).await?;
        }
        "track:add" => {
            let _ = db::set_pending_state(&db, user_id, Some(ACTION_TRACK_ADD_ADDRESS), None).await;
            send_callback_menu(
                &bot,
                &query,
                "Send the wallet address you want to track (0x...).",
                cancel_menu_markup(),
            )
            .await?;
        }
        "track:remove" => {
            let _ = db::set_pending_state(&db, user_id, Some(ACTION_TRACK_REMOVE), None).await;
            send_callback_menu(
                &bot,
                &query,
                "Send the tracked address you want to remove.",
                cancel_menu_markup(),
            )
            .await?;
        }
        "track:list" => {
            let _ = db::clear_pending_state(&db, user_id).await;
            let chat_id = query
                .message
                .as_ref()
                .map(|message| message.chat().id)
                .unwrap_or(ChatId(query.from.id.0 as i64));
            send_tracked_wallets(&bot, chat_id, &db, user_id).await?;
            bot.answer_callback_query(query.id).await?;
        }
        "track:skip_label" => {
            let chat_id = query
                .message
                .as_ref()
                .map(|message| message.chat().id)
                .unwrap_or(ChatId(query.from.id.0 as i64));
            match db::get_pending_state(&db, user_id).await {
                Ok((Some(action), data)) if action == ACTION_TRACK_ADD_LABEL => {
                    if let Some(wallet_address) = data {
                        finalize_track_add(&bot, chat_id, &db, user_id, &wallet_address, None)
                            .await?;
                    } else {
                        let _ = db::clear_pending_state(&db, user_id).await;
                        send_track_menu(&bot, chat_id).await?;
                    }
                }
                Ok(_) => {
                    let _ = db::clear_pending_state(&db, user_id).await;
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
            let chat_id = query
                .message
                .as_ref()
                .map(|message| message.chat().id)
                .unwrap_or(ChatId(query.from.id.0 as i64));
            match db::get_pending_state(&db, user_id).await {
                Ok((Some(action), data)) if action == ACTION_MANAGE_AUTH_LABEL => {
                    if let Some(wallet_address) = data {
                        finalize_manage_label(&bot, chat_id, &db, user_id, &wallet_address, None)
                            .await?;
                    } else {
                        let _ = db::clear_pending_state(&db, user_id).await;
                        send_manage_menu(&bot, chat_id).await?;
                    }
                }
                Ok(_) => {
                    let _ = db::clear_pending_state(&db, user_id).await;
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
            let _ = db::clear_pending_state(&db, user_id).await;
            send_callback_menu(
                &bot,
                &query,
                "Track wallets: add, remove, or review your list.",
                track_menu_markup(),
            )
            .await?;
        }
        "manage:cancel_action" => {
            let _ = db::clear_pending_state(&db, user_id).await;
            send_callback_menu(
                &bot,
                &query,
                "Manage your trading wallet, orders, and positions.",
                manage_menu_markup(),
            )
            .await?;
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
            send_track_menu(&bot, msg.chat.id).await?;
        }
        "manage" => {
            let _ = db::set_mode(db, user_id, "manage").await;
            bot.send_message(msg.chat.id, "Manage your trading wallet, orders, and positions.")
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
            if !is_valid_wallet_address(input) {
                bot.send_message(msg.chat.id, "That wallet address looks invalid. Expected 0x + 40 hex characters.")
                    .await?;
                return Ok(());
            }

            let wallet_address = normalize_wallet_address(input);
            if let Err(_err) = db::set_pending_state(
                db,
                user_id,
                Some(ACTION_TRACK_ADD_LABEL),
                Some(&wallet_address),
            )
            .await
            {
                bot.send_message(msg.chat.id, "Sorry, I couldn't continue that request. Try again soon.")
                    .await?;
                return Ok(());
            }
            bot.send_message(msg.chat.id, "Send a label for this wallet, or tap Skip.")
                .reply_markup(label_menu_markup())
                .await?;
        }
        ACTION_TRACK_ADD_LABEL => {
            let Some(wallet_address) = data else {
                let _ = db::clear_pending_state(db, user_id).await;
                bot.send_message(msg.chat.id, "That action expired. Use /start to open the menu.")
                    .await?;
                return Ok(());
            };

            let label = input.trim();
            if label.is_empty() {
                bot.send_message(msg.chat.id, "Send a label for this wallet, or tap Skip.")
                    .reply_markup(label_menu_markup())
                    .await?;
                return Ok(());
            }

            finalize_track_add(&bot, msg.chat.id, db, user_id, wallet_address, Some(label))
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

            let _ = db::clear_pending_state(db, user_id).await;

            if removed {
                bot.send_message(msg.chat.id, format!("Stopped tracking {wallet_address}."))
                    .await?;
            } else {
                bot.send_message(msg.chat.id, "That wallet is not being tracked.")
                    .await?;
            }

            send_track_menu(&bot, msg.chat.id).await?;
        }
        ACTION_MANAGE_AUTH_KEY => {
            let _ = bot.delete_message(msg.chat.id, msg.id).await;

            let Some(encryption_key) = encryption_key else {
                let _ = db::clear_pending_state(db, user_id).await;
                bot.send_message(msg.chat.id, "Set ENCRYPTION_KEY to store managed wallets.")
                    .await?;
                send_manage_menu(&bot, msg.chat.id).await?;
                return Ok(());
            };

            let signature_type = parse_signature_type(data);

            let private_key = input.trim();
            if private_key.is_empty() {
                bot.send_message(msg.chat.id, "Send the private key for this wallet.")
                    .reply_markup(manage_cancel_menu_markup())
                    .await?;
                return Ok(());
            }

            let signer = match LocalSigner::from_str(private_key) {
                Ok(signer) => signer.with_chain_id(Some(POLYGON)),
                Err(_) => {
                    bot.send_message(msg.chat.id, "Invalid private key format.")
                        .await?;
                    return Ok(());
                }
            };

            let wallet_address = normalize_wallet_address(&signer.address().to_string());
            let aad = crypto::build_aad(user_id, &wallet_address);
            let (encrypted_key, nonce) = match crypto::encrypt(&encryption_key, private_key.as_bytes(), &aad) {
                Ok(payload) => payload,
                Err(_err) => {
                    bot.send_message(msg.chat.id, "Sorry, I couldn't secure that key. Try again soon.")
                        .await?;
                    return Ok(());
                }
            };

            let had_wallet = matches!(db::get_managed_wallet(db, user_id).await, Ok(Some(_)));
            if let Err(_err) = db::set_managed_wallet(
                db,
                user_id,
                &wallet_address,
                &encrypted_key,
                &nonce,
                None,
                signature_type as i64,
            )
            .await
            {
                bot.send_message(msg.chat.id, "Sorry, I couldn't store that wallet. Try again soon.")
                    .await?;
                return Ok(());
            }

            if let Err(_err) = db::set_pending_state(
                db,
                user_id,
                Some(ACTION_MANAGE_AUTH_LABEL),
                Some(&wallet_address),
            )
            .await
            {
                bot.send_message(msg.chat.id, "Wallet saved. Use /start to open the menu.")
                    .await?;
                return Ok(());
            }

            if had_wallet {
                bot.send_message(msg.chat.id, format!("Wallet updated to {wallet_address}.")).await?;
            }
            bot.send_message(msg.chat.id, "Send a label for this wallet, or tap Skip.")
                .reply_markup(manage_label_menu_markup())
                .await?;
        }
        ACTION_MANAGE_AUTH_LABEL => {
            let Some(wallet_address) = data else {
                let _ = db::clear_pending_state(db, user_id).await;
                bot.send_message(msg.chat.id, "That action expired. Use /start to open the menu.")
                    .await?;
                return Ok(());
            };

            let label = input.trim();
            if label.is_empty() {
                bot.send_message(msg.chat.id, "Send a label for this wallet, or tap Skip.")
                    .reply_markup(manage_label_menu_markup())
                    .await?;
                return Ok(());
            }

            finalize_manage_label(&bot, msg.chat.id, db, user_id, wallet_address, Some(label))
                .await?;
        }
        ACTION_MANAGE_POSITIONS => {
            let _ = db::clear_pending_state(db, user_id).await;
            send_managed_positions(&bot, msg.chat.id, db, user_id).await?;
        }
        ACTION_MANAGE_MARKET_ORDER => {
            let Some(encryption_key) = encryption_key else {
                bot.send_message(msg.chat.id, "Set ENCRYPTION_KEY to place managed orders.")
                    .await?;
                return Ok(());
            };

            let parts: Vec<&str> = input.split_whitespace().collect();
            if parts.len() != 3 {
                    bot.send_message(
                        msg.chat.id,
                        "Market order format: <token_id> <buy|sell> <amount> (buy uses USDC, sell uses shares).",
                    )
                    .await?;
                return Ok(());
            }

            let token_id = match parse_token_id(parts[0]) {
                Some(token_id) => token_id,
                None => {
                    bot.send_message(msg.chat.id, "That token id looks invalid.")
                        .await?;
                    return Ok(());
                }
            };
            let side = match parse_side(parts[1]) {
                Some(side) => side,
                None => {
                    bot.send_message(msg.chat.id, "Side must be buy or sell.")
                        .await?;
                    return Ok(());
                }
            };
            let amount_value = match parse_decimal(parts[2]) {
                Some(value) => value,
                None => {
                    bot.send_message(msg.chat.id, "Amount must be a decimal number.")
                        .await?;
                    return Ok(());
                }
            };

            let (signer, signature_type) = match load_managed_wallet_signer(db, user_id, encryption_key).await {
                Ok(payload) => payload,
                Err(message) => {
                    bot.send_message(msg.chat.id, message).await?;
                    return Ok(());
                }
            };

            let client = match ClobClient::default()
                .authentication_builder(&signer)
                .signature_type(signature_type)
                .authenticate()
                .await
            {
                Ok(client) => client,
                Err(err) => {
                    let message = format!("{err}");
                    if is_wallet_type_error(&message) {
                        send_wallet_type_error(&bot, msg.chat.id, "Order failed", &message).await?;
                        let _ = db::clear_pending_state(db, user_id).await;
                        send_manage_menu(&bot, msg.chat.id).await?;
                    } else {
                        bot.send_message(msg.chat.id, "Sorry, I couldn't authenticate that wallet.")
                            .await?;
                    }
                    return Ok(());
                }
            };

            let amount = match side {
                Side::Sell => Amount::shares(amount_value),
                Side::Buy => Amount::usdc(amount_value),
                _ => Amount::usdc(amount_value),
            };
            let amount = match amount {
                Ok(amount) => amount,
                Err(_) => {
                    bot.send_message(msg.chat.id, "Amount format is not valid for this order.")
                        .await?;
                    return Ok(());
                }
            };

            let signable = match client
                .market_order()
                .token_id(token_id)
                .side(side)
                .amount(amount)
                .build()
                .await
            {
                Ok(order) => order,
                Err(_) => {
                    bot.send_message(msg.chat.id, "Sorry, I couldn't build that order.")
                        .await?;
                    return Ok(());
                }
            };

            let signed = match client.sign(&signer, signable).await {
                Ok(order) => order,
                Err(_) => {
                    bot.send_message(msg.chat.id, "Sorry, I couldn't sign that order.")
                        .await?;
                    return Ok(());
                }
            };

            let response = match client.post_order(signed).await {
                Ok(response) => response,
                Err(_) => {
                    bot.send_message(msg.chat.id, "Sorry, that order failed.")
                        .await?;
                    return Ok(());
                }
            };

            if response.success {
                bot.send_message(
                    msg.chat.id,
                    format!("Order submitted. ID: {} Status: {:?}", response.order_id, response.status),
                )
                .await?;
            } else if let Some(error) = response.error_msg {
                if is_wallet_type_error(&error) {
                    send_wallet_type_error(&bot, msg.chat.id, "Order rejected", &error).await?;
                } else {
                    bot.send_message(msg.chat.id, format!("Order rejected: {error}"))
                        .await?;
                }
            } else {
                bot.send_message(msg.chat.id, "Order rejected.").await?;
            }

            let _ = db::clear_pending_state(db, user_id).await;
            send_manage_menu(&bot, msg.chat.id).await?;
        }
        ACTION_MANAGE_LIMIT_ORDER => {
            let Some(encryption_key) = encryption_key else {
                bot.send_message(msg.chat.id, "Set ENCRYPTION_KEY to place managed orders.")
                    .await?;
                return Ok(());
            };

            let parts: Vec<&str> = input.split_whitespace().collect();
            if parts.len() != 4 {
                    bot.send_message(
                        msg.chat.id,
                        "Limit order format: <token_id> <buy|sell> <price> <size>.",
                    )
                    .await?;
                return Ok(());
            }

            let token_id = match parse_token_id(parts[0]) {
                Some(token_id) => token_id,
                None => {
                    bot.send_message(msg.chat.id, "That token id looks invalid.")
                        .await?;
                    return Ok(());
                }
            };
            let side = match parse_side(parts[1]) {
                Some(side) => side,
                None => {
                    bot.send_message(msg.chat.id, "Side must be buy or sell.")
                        .await?;
                    return Ok(());
                }
            };
            let price = match parse_decimal(parts[2]) {
                Some(value) => value,
                None => {
                    bot.send_message(msg.chat.id, "Price must be a decimal number.")
                        .await?;
                    return Ok(());
                }
            };
            let size = match parse_decimal(parts[3]) {
                Some(value) => value,
                None => {
                    bot.send_message(msg.chat.id, "Size must be a decimal number.")
                        .await?;
                    return Ok(());
                }
            };

            let (signer, signature_type) = match load_managed_wallet_signer(db, user_id, encryption_key).await {
                Ok(payload) => payload,
                Err(message) => {
                    bot.send_message(msg.chat.id, message).await?;
                    return Ok(());
                }
            };

            let client = match ClobClient::default()
                .authentication_builder(&signer)
                .signature_type(signature_type)
                .authenticate()
                .await
            {
                Ok(client) => client,
                Err(err) => {
                    let message = format!("{err}");
                    if is_wallet_type_error(&message) {
                        send_wallet_type_error(&bot, msg.chat.id, "Order failed", &message).await?;
                        let _ = db::clear_pending_state(db, user_id).await;
                        send_manage_menu(&bot, msg.chat.id).await?;
                    } else {
                        bot.send_message(msg.chat.id, "Sorry, I couldn't authenticate that wallet.")
                            .await?;
                    }
                    return Ok(());
                }
            };

            let signable = match client
                .limit_order()
                .token_id(token_id)
                .side(side)
                .price(price)
                .size(size)
                .build()
                .await
            {
                Ok(order) => order,
                Err(_) => {
                    bot.send_message(msg.chat.id, "Sorry, I couldn't build that order.")
                        .await?;
                    return Ok(());
                }
            };

            let signed = match client.sign(&signer, signable).await {
                Ok(order) => order,
                Err(_) => {
                    bot.send_message(msg.chat.id, "Sorry, I couldn't sign that order.")
                        .await?;
                    return Ok(());
                }
            };

            let response = match client.post_order(signed).await {
                Ok(response) => response,
                Err(_) => {
                    bot.send_message(msg.chat.id, "Sorry, that order failed.")
                        .await?;
                    return Ok(());
                }
            };

            if response.success {
                bot.send_message(
                    msg.chat.id,
                    format!("Order submitted. ID: {} Status: {:?}", response.order_id, response.status),
                )
                .await?;
            } else if let Some(error) = response.error_msg {
                if is_wallet_type_error(&error) {
                    send_wallet_type_error(&bot, msg.chat.id, "Order rejected", &error).await?;
                } else {
                    bot.send_message(msg.chat.id, format!("Order rejected: {error}"))
                        .await?;
                }
            } else {
                bot.send_message(msg.chat.id, "Order rejected.").await?;
            }

            let _ = db::clear_pending_state(db, user_id).await;
            send_manage_menu(&bot, msg.chat.id).await?;
        }
        ACTION_MANAGE_CANCEL_ORDER => {
            let Some(encryption_key) = encryption_key else {
                bot.send_message(msg.chat.id, "Set ENCRYPTION_KEY to cancel managed orders.")
                    .await?;
                return Ok(());
            };

            let parts: Vec<&str> = input.split_whitespace().collect();
            if parts.len() != 1 {
                bot.send_message(msg.chat.id, "Cancel format: <order_id>.")
                    .await?;
                return Ok(());
            }

            let order_id = parts[0];

            let (signer, signature_type) = match load_managed_wallet_signer(db, user_id, encryption_key).await {
                Ok(payload) => payload,
                Err(message) => {
                    bot.send_message(msg.chat.id, message).await?;
                    return Ok(());
                }
            };

            let client = match ClobClient::default()
                .authentication_builder(&signer)
                .signature_type(signature_type)
                .authenticate()
                .await
            {
                Ok(client) => client,
                Err(err) => {
                    let message = format!("{err}");
                    if is_wallet_type_error(&message) {
                        send_wallet_type_error(&bot, msg.chat.id, "Cancel failed", &message).await?;
                        let _ = db::clear_pending_state(db, user_id).await;
                        send_manage_menu(&bot, msg.chat.id).await?;
                    } else {
                        bot.send_message(msg.chat.id, "Sorry, I couldn't authenticate that wallet.")
                            .await?;
                    }
                    return Ok(());
                }
            };

            let response = match client.cancel_order(order_id).await {
                Ok(response) => response,
                Err(_) => {
                    bot.send_message(msg.chat.id, "Sorry, that cancel failed.")
                        .await?;
                    return Ok(());
                }
            };

            if response.canceled.iter().any(|id| id == order_id) {
                bot.send_message(msg.chat.id, format!("Canceled order {order_id}."))
                    .await?;
            } else if let Some(reason) = response.not_canceled.get(order_id) {
                if is_wallet_type_error(reason) {
                    send_wallet_type_error(&bot, msg.chat.id, "Cancel failed", reason).await?;
                } else {
                    bot.send_message(msg.chat.id, format!("Cancel failed: {reason}"))
                        .await?;
                }
            } else {
                bot.send_message(msg.chat.id, "Cancel failed.").await?;
            }

            let _ = db::clear_pending_state(db, user_id).await;
            send_manage_menu(&bot, msg.chat.id).await?;
        }
        _ => {
            let _ = db::clear_pending_state(db, user_id).await;
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
        InlineKeyboardButton::callback("🧭 Track", "menu:track"),
        InlineKeyboardButton::callback("⚙️ Manage", "menu:manage"),
    ]])
}

fn track_menu_markup() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![
            InlineKeyboardButton::callback("➕ Add address", "track:add"),
            InlineKeyboardButton::callback("➖ Remove address", "track:remove"),
        ],
        vec![InlineKeyboardButton::callback("📋 View all", "track:list")],
        vec![InlineKeyboardButton::callback("↩️ Back", "menu:main")],
    ])
}

fn label_menu_markup() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::callback("Skip label", "track:skip_label")],
        vec![InlineKeyboardButton::callback("Cancel", "action:cancel")],
    ])
}

fn manage_menu_markup() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![
            InlineKeyboardButton::callback("🔐 Setup wallet", "manage:auth"),
            InlineKeyboardButton::callback("👛 My wallet", "manage:list"),
        ],
        vec![InlineKeyboardButton::callback(
            "🔁 Change wallet type",
            "manage:wallet_type",
        )],
        vec![
            InlineKeyboardButton::callback("📈 Positions", "manage:positions"),
            InlineKeyboardButton::callback("⚡️ Market order", "manage:market_order"),
        ],
        vec![
            InlineKeyboardButton::callback("🎯 Limit order", "manage:limit_order"),
            InlineKeyboardButton::callback("🛑 Cancel order", "manage:cancel_order"),
        ],
        vec![InlineKeyboardButton::callback("🗑️ Remove wallet", "manage:remove")],
        vec![InlineKeyboardButton::callback("↩️ Back", "menu:main")],
    ])
}

fn manage_label_menu_markup() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::callback("Skip label", "manage:skip_label")],
        vec![InlineKeyboardButton::callback("Cancel", "manage:cancel_action")],
    ])
}

fn manage_cancel_menu_markup() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
        "Cancel",
        "manage:cancel_action",
    )]])
}

fn manage_wallet_type_setup_markup() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::callback(
            "Email/Google login (Magic)",
            "manage:auth_type_proxy",
        )],
        vec![InlineKeyboardButton::callback(
            "Standard wallet (MetaMask/Ledger)",
            "manage:auth_type_eoa",
        )],
        vec![InlineKeyboardButton::callback(
            "Cancel",
            "manage:cancel_action",
        )],
    ])
}

fn manage_wallet_type_change_markup() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::callback(
            "Email/Google login (Magic)",
            "manage:change_type_proxy",
        )],
        vec![InlineKeyboardButton::callback(
            "Standard wallet (MetaMask/Ledger)",
            "manage:change_type_eoa",
        )],
        vec![InlineKeyboardButton::callback(
            "Cancel",
            "manage:cancel_action",
        )],
    ])
}

fn manage_wallet_type_prompt_markup() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
        "Change wallet type",
        "manage:wallet_type",
    )]])
}

fn manage_remove_confirm_markup() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::callback(
            "Confirm remove",
            "manage:remove_confirm",
        )],
        vec![InlineKeyboardButton::callback(
            "Cancel",
            "manage:cancel_action",
        )],
    ])
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

async fn send_track_menu(bot: &Bot, chat_id: ChatId) -> ResponseResult<()> {
    bot.send_message(chat_id, "Track wallets: add, remove, or review your list.")
        .reply_markup(track_menu_markup())
        .await?;
    Ok(())
}

async fn finalize_track_add(
    bot: &Bot,
    chat_id: ChatId,
    db: &Db,
    user_id: i64,
    wallet_address: &str,
    label: Option<&str>,
) -> ResponseResult<()> {
    let inserted = match db::add_tracked_wallet(db, user_id, wallet_address, label).await {
        Ok(inserted) => inserted,
        Err(_err) => {
            bot.send_message(chat_id, "Sorry, I couldn't add that wallet. Try again soon.")
                .await?;
            return Ok(());
        }
    };

    let _ = db::clear_pending_state(db, user_id).await;

    if inserted {
        let response = match label {
            Some(label) => format!("Added wallet {wallet_address} as {label}.",),
            None => format!("Added wallet {wallet_address}.",),
        };
        bot.send_message(chat_id, response).await?;
    } else {
        bot.send_message(chat_id, "That wallet is already being tracked.")
            .await?;
    }

    send_track_menu(bot, chat_id).await?;
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
        bot.send_message(chat_id, "Tracking 0 wallet(s).\nNo tracked wallets yet.")
            .await?;
        return Ok(());
    }

    let mut lines = Vec::with_capacity(wallets.len());
    for wallet in wallets {
        let label_text = wallet.label.as_deref().unwrap_or("Unlabeled");
        let profile_url = format!("https://polymarket.com/profile/{}", wallet.wallet_address);
        lines.push(format!(
            "*{}* / [profile]({})\nWallet: `{}`",
            escape_markdown(label_text),
            profile_url,
            wallet.wallet_address
        ));
    }

    let message = format!(
        "👛 Tracking {} wallet(s)\n\n{}",
        lines.len(),
        lines.join("\n\n")
    );
    bot.send_message(chat_id, message)
        .parse_mode(ParseMode::MarkdownV2)
        .await?;
    Ok(())
}

async fn send_manage_menu(bot: &Bot, chat_id: ChatId) -> ResponseResult<()> {
    bot.send_message(chat_id, "Manage your trading wallet, orders, and positions.")
        .reply_markup(manage_menu_markup())
        .await?;
    Ok(())
}

async fn send_managed_positions(
    bot: &Bot,
    chat_id: ChatId,
    db: &Db,
    user_id: i64,
) -> ResponseResult<()> {
    let managed_wallet = match db::get_managed_wallet(db, user_id).await {
        Ok(wallet) => wallet,
        Err(_err) => {
            bot.send_message(chat_id, "Sorry, I couldn't load your managed wallet.")
                .await?;
            send_manage_menu(bot, chat_id).await?;
            return Ok(());
        }
    };

    let Some(managed_wallet) = managed_wallet else {
        bot.send_message(chat_id, "No wallet setup. Use Setup wallet first.")
            .await?;
        send_manage_menu(bot, chat_id).await?;
        return Ok(());
    };

    let signer_address = match Address::from_str(&managed_wallet.wallet_address) {
        Ok(address) => address,
        Err(_) => {
            bot.send_message(chat_id, "That wallet address is invalid.")
                .await?;
            send_manage_menu(bot, chat_id).await?;
            return Ok(());
        }
    };

    let signature_type = signature_type_from_db(managed_wallet.signature_type);
    let address = match signature_type {
        SignatureType::Proxy => match derive_proxy_wallet(signer_address, POLYGON) {
            Some(proxy) => proxy,
            None => {
                bot.send_message(chat_id, "Proxy wallet derivation is not supported on this chain.")
                    .await?;
                send_manage_menu(bot, chat_id).await?;
                return Ok(());
            }
        },
        _ => signer_address,
    };

    let client = DataClient::default();
    let builder = match PositionsRequest::builder().user(address).limit(200) {
        Ok(builder) => builder,
        Err(_) => {
            bot.send_message(chat_id, "Sorry, I couldn't build that request.")
                .await?;
            send_manage_menu(bot, chat_id).await?;
            return Ok(());
        }
    };
    let request = builder.build();

    let positions = match client.positions(&request).await {
        Ok(positions) => positions,
        Err(_) => {
            bot.send_message(chat_id, "Sorry, I couldn't fetch positions.")
                .await?;
            send_manage_menu(bot, chat_id).await?;
            return Ok(());
        }
    };

    let label = managed_wallet
        .label
        .as_deref()
        .unwrap_or(managed_wallet.wallet_address.as_str());
    if positions.is_empty() {
        bot.send_message(chat_id, format!("No open positions for {label}."))
            .await?;
    } else {
        let mut lines = Vec::new();
        for position in positions.iter().take(8) {
            lines.push(format!(
                "- {} ({}) size {} avg {}",
                position.title,
                position.outcome,
                format_decimal(position.size),
                format_decimal(position.avg_price)
            ));
        }
        let message = format!(
            "Open positions for {label} (showing up to 8):\n{lines}",
            lines = lines.join("\n")
        );
        bot.send_message(chat_id, message).await?;
    }

    send_manage_menu(bot, chat_id).await?;
    Ok(())
}

async fn send_managed_wallet(
    bot: &Bot,
    chat_id: ChatId,
    db: &Db,
    user_id: i64,
) -> ResponseResult<()> {
    let wallet = match db::get_managed_wallet(db, user_id).await {
        Ok(wallet) => wallet,
        Err(_err) => {
            bot.send_message(chat_id, "Sorry, I couldn't load your managed wallet.")
                .await?;
            return Ok(());
        }
    };

    let Some(wallet) = wallet else {
        bot.send_message(chat_id, "No wallet setup.").await?;
        return Ok(());
    };

    let line = match wallet.label {
        Some(label) => format!("- {} ({})", wallet.wallet_address, label),
        None => format!("- {}", wallet.wallet_address),
    };

    let signature_type = signature_type_from_db(wallet.signature_type);
    let wallet_type_label = format_signature_type(signature_type);
    let mut message = format!("Managed wallet:\n{line}\nType: {wallet_type_label}");

    if signature_type == SignatureType::Proxy {
        if let Ok(address) = Address::from_str(&wallet.wallet_address) {
            if let Some(proxy) = derive_proxy_wallet(address, POLYGON) {
                message.push_str(&format!("\nProxy address: {proxy}"));
            }
        }
    }

    bot.send_message(chat_id, message).await?;
    Ok(())
}

async fn set_managed_wallet_type(
    bot: &Bot,
    chat_id: ChatId,
    db: &Db,
    user_id: i64,
    signature_type: SignatureType,
) -> ResponseResult<()> {
    let wallet = match db::get_managed_wallet(db, user_id).await {
        Ok(wallet) => wallet,
        Err(_err) => {
            bot.send_message(chat_id, "Sorry, I couldn't load your managed wallet.")
                .await?;
            send_manage_menu(bot, chat_id).await?;
            return Ok(());
        }
    };

    let Some(_wallet) = wallet else {
        bot.send_message(chat_id, "No wallet setup. Use Setup wallet first.")
            .await?;
        send_manage_menu(bot, chat_id).await?;
        return Ok(());
    };

    let updated = match db::update_managed_wallet_signature_type(db, user_id, signature_type as i64)
        .await
    {
        Ok(updated) => updated,
        Err(_err) => {
            bot.send_message(chat_id, "Sorry, I couldn't update the wallet type.")
                .await?;
            send_manage_menu(bot, chat_id).await?;
            return Ok(());
        }
    };

    if updated {
        let label = format_signature_type(signature_type);
        bot.send_message(chat_id, format!("Wallet type set to {label}."))
            .await?;
    } else {
        bot.send_message(chat_id, "No wallet to update.").await?;
    }

    send_manage_menu(bot, chat_id).await?;
    Ok(())
}

async fn prompt_managed_wallet_removal(
    bot: &Bot,
    chat_id: ChatId,
    db: &Db,
    user_id: i64,
) -> ResponseResult<()> {
    let wallet = match db::get_managed_wallet(db, user_id).await {
        Ok(wallet) => wallet,
        Err(_err) => {
            bot.send_message(chat_id, "Sorry, I couldn't load your managed wallet.")
                .await?;
            send_manage_menu(bot, chat_id).await?;
            return Ok(());
        }
    };

    let Some(wallet) = wallet else {
        bot.send_message(chat_id, "No wallet to remove.").await?;
        send_manage_menu(bot, chat_id).await?;
        return Ok(());
    };

    let label = wallet
        .label
        .as_deref()
        .unwrap_or(wallet.wallet_address.as_str());
    bot.send_message(chat_id, format!("Remove managed wallet {label}?"))
        .reply_markup(manage_remove_confirm_markup())
        .await?;
    Ok(())
}

async fn confirm_managed_wallet_removal(
    bot: &Bot,
    chat_id: ChatId,
    db: &Db,
    user_id: i64,
) -> ResponseResult<()> {
    let wallet = match db::get_managed_wallet(db, user_id).await {
        Ok(wallet) => wallet,
        Err(_err) => {
            bot.send_message(chat_id, "Sorry, I couldn't load your managed wallet.")
                .await?;
            send_manage_menu(bot, chat_id).await?;
            return Ok(());
        }
    };

    let Some(wallet) = wallet else {
        bot.send_message(chat_id, "No wallet to remove.").await?;
        send_manage_menu(bot, chat_id).await?;
        return Ok(());
    };

    let removed = match db::remove_managed_wallet(db, user_id).await {
        Ok(removed) => removed,
        Err(_err) => {
            bot.send_message(chat_id, "Sorry, I couldn't remove that wallet. Try again soon.")
                .await?;
            send_manage_menu(bot, chat_id).await?;
            return Ok(());
        }
    };

    if removed {
        bot.send_message(
            chat_id,
            format!("Removed managed wallet {}.", wallet.wallet_address),
        )
        .await?;
    } else {
        bot.send_message(chat_id, "No wallet to remove.").await?;
    }

    send_manage_menu(bot, chat_id).await?;
    Ok(())
}

async fn finalize_manage_label(
    bot: &Bot,
    chat_id: ChatId,
    db: &Db,
    user_id: i64,
    wallet_address: &str,
    label: Option<&str>,
) -> ResponseResult<()> {
    if let Some(label) = label {
        if let Err(_err) = db::update_managed_wallet_label(db, user_id, Some(label)).await {
            bot.send_message(chat_id, "Sorry, I couldn't update that wallet.")
                .await?;
            return Ok(());
        }
    }

    let _ = db::clear_pending_state(db, user_id).await;

    let response = match label {
        Some(label) => format!("Managed wallet {wallet_address} saved as {label}."),
        None => format!("Managed wallet {wallet_address} saved."),
    };
    bot.send_message(chat_id, response).await?;
    send_manage_menu(bot, chat_id).await?;
    Ok(())
}

async fn load_managed_wallet_signer(
    db: &Db,
    user_id: i64,
    encryption_key: EncryptionKey,
) -> Result<(impl Signer, SignatureType), String> {
    let wallet = match db::get_managed_wallet(db, user_id).await {
        Ok(Some(wallet)) => wallet,
        Ok(None) => return Err("No wallet setup. Use Setup wallet first.".to_string()),
        Err(_) => return Err("Sorry, I couldn't load your managed wallet.".to_string()),
    };

    let aad = crypto::build_aad(user_id, &wallet.wallet_address);
    let decrypted = crypto::decrypt(&encryption_key, &wallet.nonce, &wallet.encrypted_key, &aad)
        .map_err(|_| "Sorry, I couldn't unlock that wallet.".to_string())?;
    let private_key = String::from_utf8(decrypted)
        .map_err(|_| "Sorry, I couldn't unlock that wallet.".to_string())?;
    let signer = LocalSigner::from_str(&private_key)
        .map_err(|_| "Sorry, I couldn't unlock that wallet.".to_string())?
        .with_chain_id(Some(POLYGON));

    let derived = normalize_wallet_address(&signer.address().to_string());
    if derived != wallet.wallet_address {
        return Err("Stored key does not match the wallet address.".to_string());
    }

    let signature_type = signature_type_from_db(wallet.signature_type);
    Ok((signer, signature_type))
}

fn parse_signature_type(data: Option<&str>) -> SignatureType {
    match data {
        Some("sig:1") => SignatureType::Proxy,
        Some("sig:2") => SignatureType::GnosisSafe,
        _ => SignatureType::Eoa,
    }
}

fn signature_type_from_db(raw: i64) -> SignatureType {
    match raw {
        1 => SignatureType::Proxy,
        2 => SignatureType::GnosisSafe,
        _ => SignatureType::Eoa,
    }
}

fn format_signature_type(signature_type: SignatureType) -> &'static str {
    match signature_type {
        SignatureType::Proxy => "Email/Google login (Magic)",
        SignatureType::GnosisSafe => "Gnosis Safe",
        SignatureType::Eoa => "Standard wallet (MetaMask/Ledger)",
        _ => "Standard wallet (MetaMask/Ledger)",
    }
}

fn is_wallet_type_error(message: &str) -> bool {
    let message = message.to_lowercase();
    message.contains("signature type")
        || message.contains("signaturetype")
        || message.contains("wallet type")
        || message.contains("proxy")
        || message.contains("funder")
        || message.contains("user type")
}

async fn send_wallet_type_error(
    bot: &Bot,
    chat_id: ChatId,
    title: &str,
    detail: &str,
) -> ResponseResult<()> {
    bot.send_message(
        chat_id,
        format!(
            "{title}: {detail}\nWallet type may be incorrect. Use Change wallet type."
        ),
    )
    .reply_markup(manage_wallet_type_prompt_markup())
    .await?;
    Ok(())
}

fn parse_side(raw: &str) -> Option<Side> {
    match raw.trim().to_lowercase().as_str() {
        "buy" => Some(Side::Buy),
        "sell" => Some(Side::Sell),
        _ => None,
    }
}

fn parse_token_id(raw: &str) -> Option<U256> {
    U256::from_str(raw.trim()).ok()
}

fn parse_decimal(raw: &str) -> Option<Decimal> {
    Decimal::from_str(raw.trim()).ok()
}

fn format_decimal(value: Decimal) -> String {
    value.normalize().to_string()
}

fn escape_markdown(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('_', "\\_")
        .replace('*', "\\*")
        .replace('[', "\\[")
        .replace(']', "\\]")
        .replace('(', "\\(")
        .replace(')', "\\)")
        .replace('~', "\\~")
        .replace('`', "\\`")
        .replace('>', "\\>")
        .replace('#', "\\#")
        .replace('+', "\\+")
        .replace('-', "\\-")
        .replace('=', "\\=")
        .replace('|', "\\|")
        .replace('{', "\\{")
        .replace('}', "\\}")
        .replace('.', "\\.")
        .replace('!', "\\!")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_signature_type_defaults_to_eoa() {
        assert_eq!(parse_signature_type(None), SignatureType::Eoa);
        assert_eq!(parse_signature_type(Some("unknown")), SignatureType::Eoa);
    }

    #[test]
    fn parse_signature_type_maps_values() {
        assert_eq!(parse_signature_type(Some("sig:0")), SignatureType::Eoa);
        assert_eq!(parse_signature_type(Some("sig:1")), SignatureType::Proxy);
        assert_eq!(parse_signature_type(Some("sig:2")), SignatureType::GnosisSafe);
    }

    #[test]
    fn signature_type_from_db_maps_values() {
        assert_eq!(signature_type_from_db(0), SignatureType::Eoa);
        assert_eq!(signature_type_from_db(1), SignatureType::Proxy);
        assert_eq!(signature_type_from_db(2), SignatureType::GnosisSafe);
        assert_eq!(signature_type_from_db(99), SignatureType::Eoa);
    }

    #[test]
    fn format_signature_type_is_user_friendly() {
        assert_eq!(format_signature_type(SignatureType::Eoa), "Standard wallet (MetaMask/Ledger)");
        assert_eq!(format_signature_type(SignatureType::Proxy), "Email/Google login (Magic)");
        assert_eq!(format_signature_type(SignatureType::GnosisSafe), "Gnosis Safe");
    }

    #[test]
    fn wallet_type_error_detection_matches_keywords() {
        assert!(is_wallet_type_error("signature type mismatch"));
        assert!(is_wallet_type_error("Proxy wallet derivation"));
        assert!(is_wallet_type_error("Cannot have a funder address"));
        assert!(is_wallet_type_error("USER TYPE invalid"));
        assert!(!is_wallet_type_error("network timeout"));
    }
}
