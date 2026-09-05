//! Copy-trade flow: preview, init, edits, and confirm.

use polymarket_client_sdk::clob::Client as ClobClient;
use polymarket_client_sdk::clob::types::{Amount, Side};
use teloxide::payloads::SendMessageSetters;
use teloxide::prelude::*;
use teloxide::types::{CallbackQuery, InlineKeyboardButton, InlineKeyboardMarkup, ParseMode};

use super::common::{MSG_ACTION_EXPIRED, log_db_error};
use super::manage::{is_wallet_type_error, load_managed_wallet_signer, send_wallet_type_error};
use super::parse::{format_decimal, html_escape, parse_decimal, parse_side, parse_token_id};
use crate::db::{self, Db};
use crate::utils::crypto::EncryptionKey;
use crate::utils::number_format;

pub(crate) fn format_copy_trade_preview(state: &db::CopyTradeState) -> String {
    let side_emoji = if state.side == "Buy" { "🟢" } else { "🔴" };
    let side_label = state.side.to_uppercase();
    let market = state.market_title.as_deref().unwrap_or("Unknown market");
    let outcome = state.outcome.as_deref().unwrap_or("N/A");

    let price_line = if state.order_type == "market" {
        "Price: at market".to_string()
    } else {
        match parse_decimal(&state.price) {
            Some(price) => format!(
                "Price: {} (limit)",
                number_format::format_price_with_odds(price)
            ),
            None => "Price: N/A (limit)".to_string(),
        }
    };

    let est_cost = if state.order_type == "market" {
        "Est. Cost: at market".to_string()
    } else {
        match (parse_decimal(&state.price), parse_decimal(&state.size)) {
            (Some(p), Some(s)) => format!("Est. Cost: {}", number_format::format_usd(p * s)),
            _ => "Est. Cost: N/A".to_string(),
        }
    };
    let size_display = parse_decimal(&state.size)
        .map(format_decimal)
        .unwrap_or_else(|| state.size.clone());

    format!(
        "{side_emoji} <b>Copy Trade</b>\n\n\
         Market: {market}\n\
         Outcome: {outcome}\n\
         Side: {side_label}\n\
         {price_line}\n\
         Shares: {size}\n\
         {est_cost}",
        market = html_escape(market),
        outcome = html_escape(outcome),
        size = size_display,
    )
}

pub(crate) fn copy_trade_markup(ct_id: i64, order_type: &str) -> InlineKeyboardMarkup {
    let toggle_label = if order_type == "market" {
        "🔄 Limit Order"
    } else {
        "🔄 Market Order"
    };

    InlineKeyboardMarkup::new(vec![
        vec![
            InlineKeyboardButton::callback("✅ Confirm", format!("ct_confirm:{ct_id}")),
            InlineKeyboardButton::callback("❌ Cancel", format!("ct_cancel:{ct_id}")),
        ],
        vec![
            InlineKeyboardButton::callback("💰 Price", format!("ct_price:{ct_id}")),
            InlineKeyboardButton::callback("📊 Size", format!("ct_size:{ct_id}")),
        ],
        vec![
            InlineKeyboardButton::callback("↕️ Flip Side", format!("ct_flip:{ct_id}")),
            InlineKeyboardButton::callback(toggle_label, format!("ct_market:{ct_id}")),
        ],
    ])
}

pub(crate) async fn send_copy_trade_preview(
    bot: &Bot,
    chat_id: ChatId,
    db: &Db,
    ct_id: i64,
) -> ResponseResult<()> {
    let state = match db::get_copy_trade_state(db, ct_id).await {
        Ok(Some(state)) => state,
        _ => {
            bot.send_message(chat_id, "Could not load copy trade state.")
                .await?;
            return Ok(());
        }
    };

    let message = format_copy_trade_preview(&state);
    let markup = copy_trade_markup(ct_id, &state.order_type);
    bot.send_message(chat_id, message)
        .parse_mode(ParseMode::Html)
        .reply_markup(markup)
        .await?;

    Ok(())
}

pub(crate) async fn handle_copy_trade_init(
    bot: &Bot,
    chat_id: ChatId,
    db: &Db,
    user_id: i64,
    cb_id: i64,
) -> ResponseResult<()> {
    let managed = match db::get_managed_wallet(db, user_id).await {
        Ok(Some(_)) => true,
        _ => false,
    };

    if !managed {
        bot.send_message(chat_id, "Set up a managed wallet first via /manage.")
            .await?;
        return Ok(());
    }

    let cb_data = match db::get_callback_data(db, cb_id).await {
        Ok(Some(data)) if data.user_id == user_id => data,
        _ => {
            bot.send_message(chat_id, "Could not load trade data.")
                .await?;
            return Ok(());
        }
    };

    let (token_id, side, price, size) = match (
        cb_data.token_id.as_deref(),
        cb_data.side.as_deref(),
        cb_data.price.as_deref(),
        cb_data.size.as_deref(),
    ) {
        (Some(t), Some(s), Some(p), Some(sz)) => (t, s, p, sz),
        _ => {
            bot.send_message(
                chat_id,
                "This activity does not have enough data to copy trade.",
            )
            .await?;
            return Ok(());
        }
    };

    let ct_id = match db::insert_copy_trade_state(
        db,
        user_id,
        token_id,
        side,
        price,
        size,
        "limit",
        cb_data.market_title.as_deref(),
        cb_data.outcome.as_deref(),
    )
    .await
    {
        Ok(id) => id,
        Err(_) => {
            bot.send_message(chat_id, "Could not create copy trade state.")
                .await?;
            return Ok(());
        }
    };

    send_copy_trade_preview(bot, chat_id, db, ct_id).await?;

    Ok(())
}

// Copy trade state rows are addressed by sequential IDs, so every read or
// mutation must verify the row belongs to the requesting user.
pub(crate) async fn load_owned_copy_trade_state(
    bot: &Bot,
    chat_id: ChatId,
    db: &Db,
    user_id: i64,
    ct_id: i64,
) -> Option<db::CopyTradeState> {
    match db::get_copy_trade_state(db, ct_id).await {
        Ok(Some(state)) if state.user_id == user_id => Some(state),
        _ => {
            let _ = bot
                .send_message(chat_id, "Copy trade session not found.")
                .await;
            None
        }
    }
}

pub(crate) async fn handle_copy_trade_flip(
    bot: &Bot,
    chat_id: ChatId,
    db: &Db,
    user_id: i64,
    ct_id: i64,
    query: &CallbackQuery,
) -> ResponseResult<()> {
    let Some(state) = load_owned_copy_trade_state(bot, chat_id, db, user_id, ct_id).await else {
        return Ok(());
    };

    let new_side = if state.side == "Buy" { "Sell" } else { "Buy" };
    log_db_error(
        db::update_copy_trade_field(db, ct_id, db::CopyTradeField::Side, new_side).await,
        "update_copy_trade_field",
        user_id,
    );

    let state = match db::get_copy_trade_state(db, ct_id).await {
        Ok(Some(state)) => state,
        _ => return Ok(()),
    };

    let message = format_copy_trade_preview(&state);
    let markup = copy_trade_markup(ct_id, &state.order_type);

    if let Some(msg) = query.message.as_ref() {
        let _ = bot
            .edit_message_text(chat_id, msg.id(), message)
            .parse_mode(ParseMode::Html)
            .reply_markup(markup)
            .await;
    }

    Ok(())
}

pub(crate) async fn handle_copy_trade_toggle_type(
    bot: &Bot,
    chat_id: ChatId,
    db: &Db,
    user_id: i64,
    ct_id: i64,
    query: &CallbackQuery,
) -> ResponseResult<()> {
    let Some(state) = load_owned_copy_trade_state(bot, chat_id, db, user_id, ct_id).await else {
        return Ok(());
    };

    let new_type = if state.order_type == "limit" {
        "market"
    } else {
        "limit"
    };
    log_db_error(
        db::update_copy_trade_field(db, ct_id, db::CopyTradeField::OrderType, new_type).await,
        "update_copy_trade_field",
        user_id,
    );

    let state = match db::get_copy_trade_state(db, ct_id).await {
        Ok(Some(state)) => state,
        _ => return Ok(()),
    };

    let message = format_copy_trade_preview(&state);
    let markup = copy_trade_markup(ct_id, &state.order_type);

    if let Some(msg) = query.message.as_ref() {
        let _ = bot
            .edit_message_text(chat_id, msg.id(), message)
            .parse_mode(ParseMode::Html)
            .reply_markup(markup)
            .await;
    }

    Ok(())
}

pub(crate) async fn handle_copy_trade_confirm(
    bot: &Bot,
    chat_id: ChatId,
    db: &Db,
    user_id: i64,
    ct_id: i64,
    encryption_key: Option<EncryptionKey>,
) -> ResponseResult<()> {
    let Some(state) = load_owned_copy_trade_state(bot, chat_id, db, user_id, ct_id).await else {
        return Ok(());
    };

    let Some(encryption_key) = encryption_key else {
        bot.send_message(chat_id, "Set ENCRYPTION_KEY to place orders.")
            .await?;
        return Ok(());
    };

    let token_id = match parse_token_id(&state.token_id) {
        Some(token_id) => token_id,
        None => {
            bot.send_message(chat_id, "Invalid token ID.").await?;
            return Ok(());
        }
    };

    let side = match parse_side(&state.side) {
        Some(side) => side,
        None => {
            bot.send_message(chat_id, "Invalid side.").await?;
            return Ok(());
        }
    };

    let (signer, signature_type) =
        match load_managed_wallet_signer(db, user_id, encryption_key).await {
            Ok(payload) => payload,
            Err(message) => {
                bot.send_message(chat_id, message).await?;
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
                send_wallet_type_error(bot, chat_id, "Order failed", &message).await?;
            } else {
                bot.send_message(chat_id, "Could not authenticate wallet.")
                    .await?;
            }
            log_db_error(
                db::delete_copy_trade_state(db, ct_id).await,
                "delete_copy_trade_state",
                user_id,
            );
            return Ok(());
        }
    };

    let response = if state.order_type == "market" {
        let amount_value = match parse_decimal(&state.size) {
            Some(value) => value,
            None => {
                bot.send_message(chat_id, "Invalid size.").await?;
                return Ok(());
            }
        };

        let amount = match side {
            Side::Sell => Amount::shares(amount_value),
            _ => Amount::usdc(amount_value),
        };
        let amount = match amount {
            Ok(amount) => amount,
            Err(_) => {
                bot.send_message(chat_id, "Invalid amount for this order.")
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
                bot.send_message(chat_id, "Could not build market order.")
                    .await?;
                return Ok(());
            }
        };

        let signed = match client.sign(&signer, signable).await {
            Ok(order) => order,
            Err(_) => {
                bot.send_message(chat_id, "Could not sign order.").await?;
                return Ok(());
            }
        };

        client.post_order(signed).await
    } else {
        let price = match parse_decimal(&state.price) {
            Some(value) => value,
            None => {
                bot.send_message(chat_id, "Invalid price.").await?;
                return Ok(());
            }
        };
        let size = match parse_decimal(&state.size) {
            Some(value) => value,
            None => {
                bot.send_message(chat_id, "Invalid size.").await?;
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
                bot.send_message(chat_id, "Could not build limit order.")
                    .await?;
                return Ok(());
            }
        };

        let signed = match client.sign(&signer, signable).await {
            Ok(order) => order,
            Err(_) => {
                bot.send_message(chat_id, "Could not sign order.").await?;
                return Ok(());
            }
        };

        client.post_order(signed).await
    };

    match response {
        Ok(response) => {
            if response.success {
                bot.send_message(
                    chat_id,
                    format!(
                        "Order submitted. ID: {} Status: {:?}",
                        response.order_id, response.status
                    ),
                )
                .await?;
            } else if let Some(error) = response.error_msg {
                if is_wallet_type_error(&error) {
                    send_wallet_type_error(bot, chat_id, "Order rejected", &error).await?;
                } else {
                    bot.send_message(chat_id, format!("Order rejected: {error}"))
                        .await?;
                }
            } else {
                bot.send_message(chat_id, "Order rejected.").await?;
            }
        }
        Err(_) => {
            bot.send_message(chat_id, "Order failed.").await?;
        }
    }

    log_db_error(
        db::delete_copy_trade_state(db, ct_id).await,
        "delete_copy_trade_state",
        user_id,
    );
    log_db_error(
        db::clear_pending_state(db, user_id).await,
        "clear_pending_state",
        user_id,
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_copy_trade_preview_limit_order() {
        let state = db::CopyTradeState {
            id: 1,
            user_id: 1,
            token_id: "12345".to_string(),
            side: "Buy".to_string(),
            price: "0.47".to_string(),
            size: "100".to_string(),
            order_type: "limit".to_string(),
            market_title: Some("Team A vs Team B".to_string()),
            outcome: Some("Team A".to_string()),
        };
        let preview = format_copy_trade_preview(&state);
        assert!(preview.contains("Copy Trade"));
        assert!(preview.contains("Team A vs Team B"));
        assert!(preview.contains("Team A"));
        assert!(preview.contains("BUY"));
        assert!(preview.contains("$0.470 (2.13)"));
        assert!(preview.contains("100.000"));
        assert!(preview.contains("limit"));
        assert!(preview.contains("$47.000"));
    }

    #[test]
    fn format_copy_trade_preview_market_order() {
        let state = db::CopyTradeState {
            id: 1,
            user_id: 1,
            token_id: "12345".to_string(),
            side: "Sell".to_string(),
            price: "0.53".to_string(),
            size: "50".to_string(),
            order_type: "market".to_string(),
            market_title: Some("Will X happen?".to_string()),
            outcome: Some("Yes".to_string()),
        };
        let preview = format_copy_trade_preview(&state);
        assert!(preview.contains("SELL"));
        assert!(preview.contains("at market"));
        assert!(!preview.contains("$0.530"));
    }

    #[test]
    fn format_copy_trade_preview_no_market_title() {
        let state = db::CopyTradeState {
            id: 1,
            user_id: 1,
            token_id: "12345".to_string(),
            side: "Buy".to_string(),
            price: "0.47".to_string(),
            size: "100".to_string(),
            order_type: "limit".to_string(),
            market_title: None,
            outcome: None,
        };
        let preview = format_copy_trade_preview(&state);
        assert!(preview.contains("Unknown market"));
        assert!(preview.contains("N/A"));
    }

    #[test]
    fn format_copy_trade_preview_html_escapes_market() {
        let state = db::CopyTradeState {
            id: 1,
            user_id: 1,
            token_id: "12345".to_string(),
            side: "Buy".to_string(),
            price: "0.50".to_string(),
            size: "10".to_string(),
            order_type: "limit".to_string(),
            market_title: Some("A <b>bold</b> & market".to_string()),
            outcome: Some("Yes".to_string()),
        };
        let preview = format_copy_trade_preview(&state);
        assert!(preview.contains("&lt;b&gt;bold&lt;/b&gt;"));
        assert!(preview.contains("&amp;"));
    }

    #[test]
    fn copy_trade_markup_limit_shows_market_toggle() {
        let markup = copy_trade_markup(42, "limit");
        let buttons: Vec<String> = markup
            .inline_keyboard
            .iter()
            .flat_map(|row| row.iter())
            .map(|btn| btn.text.clone())
            .collect();
        assert!(buttons.contains(&"🔄 Market Order".to_string()));
        assert!(buttons.contains(&"✅ Confirm".to_string()));
        assert!(buttons.contains(&"❌ Cancel".to_string()));
        assert!(buttons.contains(&"💰 Price".to_string()));
        assert!(buttons.contains(&"📊 Size".to_string()));
        assert!(buttons.contains(&"↕️ Flip Side".to_string()));
    }

    #[test]
    fn copy_trade_markup_market_shows_limit_toggle() {
        let markup = copy_trade_markup(42, "market");
        let buttons: Vec<String> = markup
            .inline_keyboard
            .iter()
            .flat_map(|row| row.iter())
            .map(|btn| btn.text.clone())
            .collect();
        assert!(buttons.contains(&"🔄 Limit Order".to_string()));
    }

    #[test]
    fn copy_trade_markup_callback_data_contains_id() {
        let markup = copy_trade_markup(99, "limit");
        let data: Vec<String> = markup
            .inline_keyboard
            .iter()
            .flat_map(|row| row.iter())
            .filter_map(|btn| match &btn.kind {
                teloxide::types::InlineKeyboardButtonKind::CallbackData(d) => Some(d.clone()),
                _ => None,
            })
            .collect();
        assert!(data.contains(&"ct_confirm:99".to_string()));
        assert!(data.contains(&"ct_cancel:99".to_string()));
        assert!(data.contains(&"ct_flip:99".to_string()));
        assert!(data.contains(&"ct_market:99".to_string()));
        assert!(data.contains(&"ct_price:99".to_string()));
        assert!(data.contains(&"ct_size:99".to_string()));
    }
}

pub(crate) async fn handle_price_input(
    bot: &Bot,
    msg: &Message,
    db: &Db,
    user_id: i64,
    data: Option<&str>,
    input: &str,
) -> ResponseResult<()> {
    let Some(ct_id_str) = data else {
        log_db_error(
            db::clear_pending_state(db, user_id).await,
            "clear_pending_state",
            user_id,
        );
        bot.send_message(msg.chat.id, MSG_ACTION_EXPIRED).await?;
        return Ok(());
    };
    let ct_id = match ct_id_str.parse::<i64>() {
        Ok(id) => id,
        Err(_) => {
            log_db_error(
                db::clear_pending_state(db, user_id).await,
                "clear_pending_state",
                user_id,
            );
            bot.send_message(msg.chat.id, MSG_ACTION_EXPIRED).await?;
            return Ok(());
        }
    };
    let price = match parse_decimal(input) {
        Some(_) => input.trim(),
        None => {
            bot.send_message(msg.chat.id, "Price must be a decimal number (e.g., 0.47).")
                .await?;
            return Ok(());
        }
    };
    if load_owned_copy_trade_state(&bot, msg.chat.id, db, user_id, ct_id)
        .await
        .is_none()
    {
        log_db_error(
            db::clear_pending_state(db, user_id).await,
            "clear_pending_state",
            user_id,
        );
        return Ok(());
    }
    log_db_error(
        db::update_copy_trade_field(db, ct_id, db::CopyTradeField::Price, price).await,
        "update_copy_trade_field",
        user_id,
    );
    log_db_error(
        db::clear_pending_state(db, user_id).await,
        "clear_pending_state",
        user_id,
    );
    send_copy_trade_preview(&bot, msg.chat.id, db, ct_id).await?;
    Ok(())
}

pub(crate) async fn handle_size_input(
    bot: &Bot,
    msg: &Message,
    db: &Db,
    user_id: i64,
    data: Option<&str>,
    input: &str,
) -> ResponseResult<()> {
    let Some(ct_id_str) = data else {
        log_db_error(
            db::clear_pending_state(db, user_id).await,
            "clear_pending_state",
            user_id,
        );
        bot.send_message(msg.chat.id, MSG_ACTION_EXPIRED).await?;
        return Ok(());
    };
    let ct_id = match ct_id_str.parse::<i64>() {
        Ok(id) => id,
        Err(_) => {
            log_db_error(
                db::clear_pending_state(db, user_id).await,
                "clear_pending_state",
                user_id,
            );
            bot.send_message(msg.chat.id, MSG_ACTION_EXPIRED).await?;
            return Ok(());
        }
    };
    let size = match parse_decimal(input) {
        Some(_) => input.trim(),
        None => {
            bot.send_message(msg.chat.id, "Size must be a number.")
                .await?;
            return Ok(());
        }
    };
    if load_owned_copy_trade_state(&bot, msg.chat.id, db, user_id, ct_id)
        .await
        .is_none()
    {
        log_db_error(
            db::clear_pending_state(db, user_id).await,
            "clear_pending_state",
            user_id,
        );
        return Ok(());
    }
    log_db_error(
        db::update_copy_trade_field(db, ct_id, db::CopyTradeField::Size, size).await,
        "update_copy_trade_field",
        user_id,
    );
    log_db_error(
        db::clear_pending_state(db, user_id).await,
        "clear_pending_state",
        user_id,
    );
    send_copy_trade_preview(&bot, msg.chat.id, db, ct_id).await?;
    Ok(())
}
