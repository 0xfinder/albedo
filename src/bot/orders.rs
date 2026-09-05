//! Managed-wallet order flows: market, limit, and cancel inputs.

use polymarket_client_sdk::clob::Client as ClobClient;
use polymarket_client_sdk::clob::types::{Amount, Side};
use teloxide::prelude::*;

use super::common::log_db_error;
use super::manage::{
    is_wallet_type_error, load_managed_wallet_signer, send_manage_menu, send_wallet_type_error,
};
use super::parse::{parse_decimal, parse_side, parse_token_id};
use crate::db::{self, Db};
use crate::utils::crypto::EncryptionKey;

pub(crate) async fn handle_market_order_input(
    bot: &Bot,
    msg: &Message,
    db: &Db,
    user_id: i64,
    input: &str,
    encryption_key: Option<EncryptionKey>,
) -> ResponseResult<()> {
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

    let (signer, signature_type) =
        match load_managed_wallet_signer(db, user_id, encryption_key).await {
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
                log_db_error(
                    db::clear_pending_state(db, user_id).await,
                    "clear_pending_state",
                    user_id,
                );
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
            format!(
                "Order submitted. ID: {} Status: {:?}",
                response.order_id, response.status
            ),
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

    log_db_error(
        db::clear_pending_state(db, user_id).await,
        "clear_pending_state",
        user_id,
    );
    send_manage_menu(&bot, msg.chat.id).await?;
    Ok(())
}

pub(crate) async fn handle_limit_order_input(
    bot: &Bot,
    msg: &Message,
    db: &Db,
    user_id: i64,
    input: &str,
    encryption_key: Option<EncryptionKey>,
) -> ResponseResult<()> {
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

    let (signer, signature_type) =
        match load_managed_wallet_signer(db, user_id, encryption_key).await {
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
                log_db_error(
                    db::clear_pending_state(db, user_id).await,
                    "clear_pending_state",
                    user_id,
                );
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
            format!(
                "Order submitted. ID: {} Status: {:?}",
                response.order_id, response.status
            ),
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

    log_db_error(
        db::clear_pending_state(db, user_id).await,
        "clear_pending_state",
        user_id,
    );
    send_manage_menu(&bot, msg.chat.id).await?;
    Ok(())
}

pub(crate) async fn handle_cancel_order_input(
    bot: &Bot,
    msg: &Message,
    db: &Db,
    user_id: i64,
    input: &str,
    encryption_key: Option<EncryptionKey>,
) -> ResponseResult<()> {
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

    let (signer, signature_type) =
        match load_managed_wallet_signer(db, user_id, encryption_key).await {
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
                log_db_error(
                    db::clear_pending_state(db, user_id).await,
                    "clear_pending_state",
                    user_id,
                );
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

    log_db_error(
        db::clear_pending_state(db, user_id).await,
        "clear_pending_state",
        user_id,
    );
    send_manage_menu(&bot, msg.chat.id).await?;
    Ok(())
}
