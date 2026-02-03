use std::str::FromStr;

use polymarket_client_sdk::auth::{LocalSigner, Signer};
use polymarket_client_sdk::clob::types::{Amount, Side};
use polymarket_client_sdk::clob::Client as ClobClient;
use polymarket_client_sdk::data::types::request::PositionsRequest;
use polymarket_client_sdk::data::Client as DataClient;
use polymarket_client_sdk::types::{Address, Decimal, U256};
use polymarket_client_sdk::POLYGON;
use teloxide::prelude::*;
use teloxide::types::{
    CallbackQuery, InlineKeyboardButton, InlineKeyboardMarkup, KeyboardRemove,
};
use teloxide::utils::command::parse_command;

use crate::db::{self, Db};
use crate::utils::crypto::{self, EncryptionKey};

const HELP_TEXT: &str = "Available commands:\n\
/start - Start the bot\n\
/help - Show this help message";

const ACTION_TRACK_ADD_ADDRESS: &str = "track_add_address";
const ACTION_TRACK_ADD_LABEL: &str = "track_add_label";
const ACTION_TRACK_REMOVE: &str = "track_remove";
const ACTION_MANAGE_AUTH_KEY: &str = "manage_auth_key";
const ACTION_MANAGE_AUTH_LABEL: &str = "manage_auth_label";
const ACTION_MANAGE_REMOVE: &str = "manage_remove";
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
            send_callback_menu(&bot, &query, "Choose a mode:", main_menu_markup()).await?;
        }
        "menu:track" => {
            let _ = db::clear_pending_state(&db, user_id).await;
            let _ = db::set_mode(&db, user_id, "track").await;
            send_callback_menu(&bot, &query, "Track menu:", track_menu_markup()).await?;
        }
        "menu:manage" => {
            let _ = db::clear_pending_state(&db, user_id).await;
            let _ = db::set_mode(&db, user_id, "manage").await;
            send_callback_menu(&bot, &query, "Manage menu:", manage_menu_markup()).await?;
        }
        "manage:auth" => {
            let _ = db::set_pending_state(&db, user_id, Some(ACTION_MANAGE_AUTH_KEY), None).await;
            send_callback_menu(
                &bot,
                &query,
                "Send the private key for this wallet.",
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
            send_managed_wallets(&bot, chat_id, &db, user_id).await?;
            bot.answer_callback_query(query.id).await?;
        }
        "manage:positions" => {
            let _ = db::set_pending_state(&db, user_id, Some(ACTION_MANAGE_POSITIONS), None).await;
            send_callback_menu(
                &bot,
                &query,
                "Send the managed wallet address to view positions.",
                manage_cancel_menu_markup(),
            )
            .await?;
        }
        "manage:market_order" => {
            let _ = db::set_pending_state(&db, user_id, Some(ACTION_MANAGE_MARKET_ORDER), None)
                .await;
            send_callback_menu(
                &bot,
                &query,
                "Send: <wallet> <token_id> <side> <amount> (buy uses USDC, sell uses shares).",
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
                "Send: <wallet> <token_id> <side> <price> <size>.",
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
                "Send: <wallet> <order_id>.",
                manage_cancel_menu_markup(),
            )
            .await?;
        }
        "manage:remove" => {
            let _ = db::set_pending_state(&db, user_id, Some(ACTION_MANAGE_REMOVE), None).await;
            send_callback_menu(
                &bot,
                &query,
                "Send the managed wallet address to remove.",
                manage_cancel_menu_markup(),
            )
            .await?;
        }
        "track:add" => {
            let _ = db::set_pending_state(&db, user_id, Some(ACTION_TRACK_ADD_ADDRESS), None).await;
            send_callback_menu(
                &bot,
                &query,
                "Send the wallet address to track.",
                cancel_menu_markup(),
            )
            .await?;
        }
        "track:remove" => {
            let _ = db::set_pending_state(&db, user_id, Some(ACTION_TRACK_REMOVE), None).await;
            send_callback_menu(
                &bot,
                &query,
                "Send the wallet address to remove.",
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
        "track:status" => {
            let _ = db::clear_pending_state(&db, user_id).await;
            let chat_id = query
                .message
                .as_ref()
                .map(|message| message.chat().id)
                .unwrap_or(ChatId(query.from.id.0 as i64));
            send_track_status(&bot, chat_id, &db, user_id).await?;
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
            send_callback_menu(&bot, &query, "Track menu:", track_menu_markup()).await?;
        }
        "manage:cancel_action" => {
            let _ = db::clear_pending_state(&db, user_id).await;
            send_callback_menu(&bot, &query, "Manage menu:", manage_menu_markup()).await?;
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
            bot.send_message(msg.chat.id, "Manage menu:")
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
            let Some(encryption_key) = encryption_key else {
                let _ = db::clear_pending_state(db, user_id).await;
                bot.send_message(msg.chat.id, "Set ENCRYPTION_KEY to store managed wallets.")
                    .await?;
                send_manage_menu(&bot, msg.chat.id).await?;
                return Ok(());
            };

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
                    bot.send_message(msg.chat.id, "That private key looks invalid.")
                        .await?;
                    return Ok(());
                }
            };

            let wallet_address = normalize_wallet_address(&signer.address().to_string());
            let (encrypted_key, nonce) = match crypto::encrypt(encryption_key, private_key.as_bytes()) {
                Ok(payload) => payload,
                Err(_err) => {
                    bot.send_message(msg.chat.id, "Sorry, I couldn't secure that key. Try again soon.")
                        .await?;
                    return Ok(());
                }
            };

            if let Err(_err) = db::upsert_managed_wallet(
                db,
                user_id,
                &wallet_address,
                &encrypted_key,
                &nonce,
                None,
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
        ACTION_MANAGE_REMOVE => {
            if !is_valid_wallet_address(input) {
                bot.send_message(msg.chat.id, "That wallet address looks invalid. Expected 0x + 40 hex characters.")
                    .await?;
                return Ok(());
            }

            let wallet_address = normalize_wallet_address(input);
            let removed = match db::remove_managed_wallet(db, user_id, &wallet_address).await {
                Ok(removed) => removed,
                Err(_err) => {
                    bot.send_message(msg.chat.id, "Sorry, I couldn't remove that wallet. Try again soon.")
                        .await?;
                    return Ok(());
                }
            };

            let _ = db::clear_pending_state(db, user_id).await;

            if removed {
                bot.send_message(msg.chat.id, format!("Removed managed wallet {wallet_address}."))
                    .await?;
            } else {
                bot.send_message(msg.chat.id, "That wallet is not managed yet.")
                    .await?;
            }

            send_manage_menu(&bot, msg.chat.id).await?;
        }
        ACTION_MANAGE_POSITIONS => {
            if !is_valid_wallet_address(input) {
                bot.send_message(msg.chat.id, "That wallet address looks invalid. Expected 0x + 40 hex characters.")
                    .await?;
                return Ok(());
            }

            let wallet_address = normalize_wallet_address(input);
            let managed_wallet = match db::get_managed_wallet(db, user_id, &wallet_address).await {
                Ok(Some(wallet)) => wallet,
                Ok(None) => {
                    bot.send_message(msg.chat.id, "That wallet is not managed yet.")
                        .await?;
                    return Ok(());
                }
                Err(_err) => {
                    bot.send_message(msg.chat.id, "Sorry, I couldn't load that wallet.")
                        .await?;
                    return Ok(());
                }
            };

            let address = match Address::from_str(&managed_wallet.wallet_address) {
                Ok(address) => address,
                Err(_) => {
                    bot.send_message(msg.chat.id, "That wallet address is invalid.")
                        .await?;
                    return Ok(());
                }
            };

            let client = DataClient::default();
            let builder = match PositionsRequest::builder().user(address).limit(200) {
                Ok(builder) => builder,
                Err(_) => {
                    bot.send_message(msg.chat.id, "Sorry, I couldn't build that request.")
                        .await?;
                    return Ok(());
                }
            };
            let request = builder.build();

            let positions = match client.positions(&request).await {
                Ok(positions) => positions,
                Err(_) => {
                    bot.send_message(msg.chat.id, "Sorry, I couldn't fetch positions.")
                        .await?;
                    return Ok(());
                }
            };

            let label = managed_wallet
                .label
                .as_deref()
                .unwrap_or(managed_wallet.wallet_address.as_str());
            if positions.is_empty() {
                bot.send_message(msg.chat.id, format!("No open positions for {label}."))
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
                    "Open positions for {label} ({count}):\n{lines}",
                    count = positions.len(),
                    lines = lines.join("\n")
                );
                bot.send_message(msg.chat.id, message).await?;
            }

            let _ = db::clear_pending_state(db, user_id).await;
            send_manage_menu(&bot, msg.chat.id).await?;
        }
        ACTION_MANAGE_MARKET_ORDER => {
            let Some(encryption_key) = encryption_key else {
                bot.send_message(msg.chat.id, "Set ENCRYPTION_KEY to place managed orders.")
                    .await?;
                return Ok(());
            };

            let parts: Vec<&str> = input.split_whitespace().collect();
            if parts.len() != 4 {
                bot.send_message(
                    msg.chat.id,
                    "Send: <wallet> <token_id> <side> <amount> (buy uses USDC, sell uses shares).",
                )
                .await?;
                return Ok(());
            }

            let wallet_address = parts[0];
            if !is_valid_wallet_address(wallet_address) {
                bot.send_message(msg.chat.id, "That wallet address looks invalid.")
                    .await?;
                return Ok(());
            }
            let wallet_address = normalize_wallet_address(wallet_address);
            let token_id = match parse_token_id(parts[1]) {
                Some(token_id) => token_id,
                None => {
                    bot.send_message(msg.chat.id, "That token id looks invalid.")
                        .await?;
                    return Ok(());
                }
            };
            let side = match parse_side(parts[2]) {
                Some(side) => side,
                None => {
                    bot.send_message(msg.chat.id, "Side must be buy or sell.")
                        .await?;
                    return Ok(());
                }
            };
            let amount_value = match parse_decimal(parts[3]) {
                Some(value) => value,
                None => {
                    bot.send_message(msg.chat.id, "Amount must be a decimal number.")
                        .await?;
                    return Ok(());
                }
            };

            let signer = match load_managed_wallet_signer(db, user_id, &wallet_address, encryption_key).await {
                Ok(signer) => signer,
                Err(message) => {
                    bot.send_message(msg.chat.id, message).await?;
                    return Ok(());
                }
            };

            let client = match ClobClient::default().authentication_builder(&signer).authenticate().await {
                Ok(client) => client,
                Err(_) => {
                    bot.send_message(msg.chat.id, "Sorry, I couldn't authenticate that wallet.")
                        .await?;
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
                bot.send_message(msg.chat.id, format!("Order rejected: {error}"))
                    .await?;
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
            if parts.len() != 5 {
                bot.send_message(
                    msg.chat.id,
                    "Send: <wallet> <token_id> <side> <price> <size>.",
                )
                .await?;
                return Ok(());
            }

            let wallet_address = parts[0];
            if !is_valid_wallet_address(wallet_address) {
                bot.send_message(msg.chat.id, "That wallet address looks invalid.")
                    .await?;
                return Ok(());
            }
            let wallet_address = normalize_wallet_address(wallet_address);
            let token_id = match parse_token_id(parts[1]) {
                Some(token_id) => token_id,
                None => {
                    bot.send_message(msg.chat.id, "That token id looks invalid.")
                        .await?;
                    return Ok(());
                }
            };
            let side = match parse_side(parts[2]) {
                Some(side) => side,
                None => {
                    bot.send_message(msg.chat.id, "Side must be buy or sell.")
                        .await?;
                    return Ok(());
                }
            };
            let price = match parse_decimal(parts[3]) {
                Some(value) => value,
                None => {
                    bot.send_message(msg.chat.id, "Price must be a decimal number.")
                        .await?;
                    return Ok(());
                }
            };
            let size = match parse_decimal(parts[4]) {
                Some(value) => value,
                None => {
                    bot.send_message(msg.chat.id, "Size must be a decimal number.")
                        .await?;
                    return Ok(());
                }
            };

            let signer = match load_managed_wallet_signer(db, user_id, &wallet_address, encryption_key).await {
                Ok(signer) => signer,
                Err(message) => {
                    bot.send_message(msg.chat.id, message).await?;
                    return Ok(());
                }
            };

            let client = match ClobClient::default().authentication_builder(&signer).authenticate().await {
                Ok(client) => client,
                Err(_) => {
                    bot.send_message(msg.chat.id, "Sorry, I couldn't authenticate that wallet.")
                        .await?;
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
                bot.send_message(msg.chat.id, format!("Order rejected: {error}"))
                    .await?;
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
            if parts.len() != 2 {
                bot.send_message(msg.chat.id, "Send: <wallet> <order_id>.")
                    .await?;
                return Ok(());
            }

            let wallet_address = parts[0];
            if !is_valid_wallet_address(wallet_address) {
                bot.send_message(msg.chat.id, "That wallet address looks invalid.")
                    .await?;
                return Ok(());
            }
            let wallet_address = normalize_wallet_address(wallet_address);
            let order_id = parts[1];

            let signer = match load_managed_wallet_signer(db, user_id, &wallet_address, encryption_key).await {
                Ok(signer) => signer,
                Err(message) => {
                    bot.send_message(msg.chat.id, message).await?;
                    return Ok(());
                }
            };

            let client = match ClobClient::default().authentication_builder(&signer).authenticate().await {
                Ok(client) => client,
                Err(_) => {
                    bot.send_message(msg.chat.id, "Sorry, I couldn't authenticate that wallet.")
                        .await?;
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
                bot.send_message(msg.chat.id, format!("Cancel failed: {reason}"))
                    .await?;
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

fn label_menu_markup() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::callback("Skip label", "track:skip_label")],
        vec![InlineKeyboardButton::callback("Cancel", "action:cancel")],
    ])
}

fn manage_menu_markup() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![
            InlineKeyboardButton::callback("Auth wallet", "manage:auth"),
            InlineKeyboardButton::callback("List wallets", "manage:list"),
        ],
        vec![
            InlineKeyboardButton::callback("Positions", "manage:positions"),
            InlineKeyboardButton::callback("Market order", "manage:market_order"),
        ],
        vec![
            InlineKeyboardButton::callback("Limit order", "manage:limit_order"),
            InlineKeyboardButton::callback("Cancel order", "manage:cancel_order"),
        ],
        vec![InlineKeyboardButton::callback("Remove wallet", "manage:remove")],
        vec![InlineKeyboardButton::callback("Back", "menu:main")],
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
    bot.send_message(chat_id, "Track menu:")
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

    bot.send_message(chat_id, format!("Tracking {count} wallet(s)."))
        .await?;
    Ok(())
}

async fn send_manage_menu(bot: &Bot, chat_id: ChatId) -> ResponseResult<()> {
    bot.send_message(chat_id, "Manage menu:")
        .reply_markup(manage_menu_markup())
        .await?;
    Ok(())
}

async fn send_managed_wallets(
    bot: &Bot,
    chat_id: ChatId,
    db: &Db,
    user_id: i64,
) -> ResponseResult<()> {
    let wallets = match db::list_managed_wallets(db, user_id).await {
        Ok(wallets) => wallets,
        Err(_err) => {
            bot.send_message(chat_id, "Sorry, I couldn't load your managed wallets.")
                .await?;
            return Ok(());
        }
    };

    if wallets.is_empty() {
        bot.send_message(chat_id, "No managed wallets yet.").await?;
        return Ok(());
    }

    let mut lines = Vec::with_capacity(wallets.len());
    for wallet in wallets {
        match wallet.label {
            Some(label) => lines.push(format!("- {} ({})", wallet.wallet_address, label)),
            None => lines.push(format!("- {}", wallet.wallet_address)),
        }
    }

    bot.send_message(chat_id, format!("Managed wallets:\n{}", lines.join("\n")))
        .await?;
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
        if let Err(_err) =
            db::update_managed_wallet_label(db, user_id, wallet_address, Some(label)).await
        {
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
    wallet_address: &str,
    encryption_key: EncryptionKey,
) -> Result<impl Signer, String> {
    let wallet = match db::get_managed_wallet(db, user_id, wallet_address).await {
        Ok(Some(wallet)) => wallet,
        Ok(None) => return Err("That wallet is not managed yet.".to_string()),
        Err(_) => return Err("Sorry, I couldn't load that wallet.".to_string()),
    };

    let decrypted = crypto::decrypt(encryption_key, &wallet.nonce, &wallet.encrypted_key)
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

    Ok(signer)
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
