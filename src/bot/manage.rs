//! Managed-wallet menus: setup, positions, orders, and removal.

use std::str::FromStr;

use polymarket_client_sdk::auth::{LocalSigner, Signer};
use polymarket_client_sdk::clob::types::SignatureType;
use polymarket_client_sdk::data::types::MarketFilter;
use polymarket_client_sdk::data::types::request::PositionsRequest;
use polymarket_client_sdk::data::types::response::Position;
use polymarket_client_sdk::types::{Address, B256, Decimal};
use polymarket_client_sdk::{POLYGON, derive_proxy_wallet};
use teloxide::payloads::SendMessageSetters;
use teloxide::prelude::*;
use teloxide::types::ParseMode;
use zeroize::Zeroizing;

use super::common::{
    ACTION_MANAGE_AUTH_LABEL, MSG_ACTION_EXPIRED, MSG_SEND_LABEL_SKIP, MSG_WALLET_LOAD_FAILED,
    POSITIONS_DISPLAY_LIMIT, POSITIONS_PAGE_LIMIT, data_client, log_db_error,
};
use super::menus::{
    manage_cancel_menu_markup, manage_label_menu_markup, manage_menu_markup,
    manage_remove_confirm_markup, manage_wallet_type_prompt_markup,
};
use super::parse::{
    extract_wallet_address_from_text, format_decimal, format_signature_type, format_signed_usd,
    format_value_change, html_escape, normalize_wallet_address, parse_signature_type,
    signature_type_from_db,
};
use crate::db::{self, Db};
use crate::utils::crypto::{self, EncryptionKey};
use crate::utils::number_format;

pub(crate) async fn send_manage_menu(bot: &Bot, chat_id: ChatId) -> ResponseResult<()> {
    bot.send_message(
        chat_id,
        "Manage your trading wallet, orders, and positions.",
    )
    .reply_markup(manage_menu_markup())
    .await?;
    Ok(())
}

pub(crate) async fn send_managed_positions(
    bot: &Bot,
    chat_id: ChatId,
    db: &Db,
    user_id: i64,
) -> ResponseResult<()> {
    let managed_wallet = match db::get_managed_wallet(db, user_id).await {
        Ok(wallet) => wallet,
        Err(_err) => {
            bot.send_message(chat_id, MSG_WALLET_LOAD_FAILED).await?;
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
                bot.send_message(
                    chat_id,
                    "Proxy wallet derivation is not supported on this chain.",
                )
                .await?;
                send_manage_menu(bot, chat_id).await?;
                return Ok(());
            }
        },
        _ => signer_address,
    };

    let client = data_client();
    let builder = match PositionsRequest::builder()
        .user(address)
        .limit(POSITIONS_PAGE_LIMIT)
    {
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
        for position in positions.iter().take(POSITIONS_DISPLAY_LIMIT) {
            lines.push(format!(
                "- {} ({}) size {} avg {}",
                position.title,
                position.outcome,
                format_decimal(position.size),
                number_format::format_price_with_odds(position.avg_price)
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

pub(crate) async fn send_managed_wallet(
    bot: &Bot,
    chat_id: ChatId,
    db: &Db,
    user_id: i64,
) -> ResponseResult<()> {
    let wallet = match db::get_managed_wallet(db, user_id).await {
        Ok(wallet) => wallet,
        Err(_err) => {
            bot.send_message(chat_id, MSG_WALLET_LOAD_FAILED).await?;
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

pub(crate) async fn set_managed_wallet_type(
    bot: &Bot,
    chat_id: ChatId,
    db: &Db,
    user_id: i64,
    signature_type: SignatureType,
) -> ResponseResult<()> {
    let wallet = match db::get_managed_wallet(db, user_id).await {
        Ok(wallet) => wallet,
        Err(_err) => {
            bot.send_message(chat_id, MSG_WALLET_LOAD_FAILED).await?;
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

    let updated =
        match db::update_managed_wallet_signature_type(db, user_id, signature_type as i64).await {
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

pub(crate) async fn prompt_managed_wallet_removal(
    bot: &Bot,
    chat_id: ChatId,
    db: &Db,
    user_id: i64,
) -> ResponseResult<()> {
    let wallet = match db::get_managed_wallet(db, user_id).await {
        Ok(wallet) => wallet,
        Err(_err) => {
            bot.send_message(chat_id, MSG_WALLET_LOAD_FAILED).await?;
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

pub(crate) async fn confirm_managed_wallet_removal(
    bot: &Bot,
    chat_id: ChatId,
    db: &Db,
    user_id: i64,
) -> ResponseResult<()> {
    let wallet = match db::get_managed_wallet(db, user_id).await {
        Ok(wallet) => wallet,
        Err(_err) => {
            bot.send_message(chat_id, MSG_WALLET_LOAD_FAILED).await?;
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
            bot.send_message(
                chat_id,
                "Sorry, I couldn't remove that wallet. Try again soon.",
            )
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

pub(crate) async fn finalize_manage_label(
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

    log_db_error(
        db::clear_pending_state(db, user_id).await,
        "clear_pending_state",
        user_id,
    );

    let response = match label {
        Some(label) => format!("Managed wallet {wallet_address} saved as {label}."),
        None => format!("Managed wallet {wallet_address} saved."),
    };
    bot.send_message(chat_id, response).await?;
    send_manage_menu(bot, chat_id).await?;
    Ok(())
}

pub(crate) async fn load_managed_wallet_signer(
    db: &Db,
    user_id: i64,
    encryption_key: EncryptionKey,
) -> Result<(impl Signer, SignatureType), String> {
    let wallet = match db::get_managed_wallet(db, user_id).await {
        Ok(Some(wallet)) => wallet,
        Ok(None) => return Err("No wallet setup. Use Setup wallet first.".to_string()),
        Err(_) => return Err(MSG_WALLET_LOAD_FAILED.to_string()),
    };

    let aad = crypto::build_aad(user_id, &wallet.wallet_address);
    let decrypted = Zeroizing::new(
        crypto::decrypt(&encryption_key, &wallet.nonce, &wallet.encrypted_key, &aad)
            .map_err(|_| "Sorry, I couldn't unlock that wallet.".to_string())?,
    );
    let private_key = Zeroizing::new(
        String::from_utf8(decrypted.to_vec())
            .map_err(|_| "Sorry, I couldn't unlock that wallet.".to_string())?,
    );
    let signer = LocalSigner::from_str(&private_key)
        .map_err(|_| "Sorry, I couldn't unlock that wallet.".to_string())?
        .with_chain_id(Some(POLYGON));
    drop(private_key);

    let derived = normalize_wallet_address(&signer.address().to_string());
    if derived != wallet.wallet_address {
        return Err("Stored key does not match the wallet address.".to_string());
    }

    let signature_type = signature_type_from_db(wallet.signature_type);
    Ok((signer, signature_type))
}

pub(crate) fn is_wallet_type_error(message: &str) -> bool {
    let message = message.to_lowercase();
    message.contains("signature type")
        || message.contains("signaturetype")
        || message.contains("wallet type")
        || message.contains("proxy")
        || message.contains("funder")
        || message.contains("user type")
}

pub(crate) async fn send_wallet_type_error(
    bot: &Bot,
    chat_id: ChatId,
    title: &str,
    detail: &str,
) -> ResponseResult<()> {
    bot.send_message(
        chat_id,
        format!("{title}: {detail}\nWallet type may be incorrect. Use Change wallet type."),
    )
    .reply_markup(manage_wallet_type_prompt_markup())
    .await?;
    Ok(())
}

pub(crate) fn format_position_message(
    label: &str,
    size: Decimal,
    avg_price: Decimal,
    cur_price: Decimal,
    cash_pnl: Decimal,
) -> String {
    let size_display = format_decimal(size);
    let avg_display = number_format::format_price_with_odds(avg_price);
    let cur_display = number_format::format_price_with_odds(cur_price);
    let purchased_value = size * avg_price;
    let current_value = size * cur_price;
    let purchased_value_display = number_format::format_usd(purchased_value);
    let current_value_display = number_format::format_usd(current_value);
    let value_change = format_value_change(cash_pnl, purchased_value);

    format!(
        "• <b>{label}</b>\n<b>Size:</b> {size_display}\n<b>Price:</b> {avg_display} → {cur_display}\n<b>Value:</b> {purchased_value_display} → {current_value_display} {value_change}"
    )
}

#[derive(Clone)]
pub(crate) struct OutcomeExposure {
    outcome: String,
    size: Decimal,
    cost: Decimal,
}

pub(crate) fn format_directional_summary(
    long_outcome: &str,
    long_size: Decimal,
    long_avg_price: Decimal,
    short_outcome: &str,
    short_size: Decimal,
    short_avg_price: Decimal,
) -> String {
    let hedged_size = long_size.min(short_size);
    let net_size = long_size - short_size;
    let hedge_carry = hedged_size * (Decimal::ONE - long_avg_price - short_avg_price);
    let if_long_wins = hedge_carry + net_size * (Decimal::ONE - long_avg_price);
    let if_short_wins = hedge_carry - net_size * long_avg_price;

    format!(
        "<b>Directional Summary</b>\n<b>Direction:</b> {long_outcome} +{}\n<b>Hedged:</b> {}\n<b>Hedge Carry:</b> {}\n<b>If {long_outcome} wins:</b> {}\n<b>If {short_outcome} wins:</b> {}",
        format_decimal(net_size),
        format_decimal(hedged_size),
        format_signed_usd(hedge_carry),
        format_signed_usd(if_long_wins),
        format_signed_usd(if_short_wins),
    )
}

pub(crate) fn build_directional_summary(positions: &[&Position]) -> Option<String> {
    let mut exposures: Vec<OutcomeExposure> = Vec::new();

    for pos in positions {
        let cost = pos.size * pos.avg_price;
        if let Some(existing) = exposures
            .iter_mut()
            .find(|entry| entry.outcome == pos.outcome)
        {
            existing.size += pos.size;
            existing.cost += cost;
        } else {
            exposures.push(OutcomeExposure {
                outcome: pos.outcome.clone(),
                size: pos.size,
                cost,
            });
        }
    }

    if exposures.len() != 2 {
        return None;
    }

    let first = exposures.remove(0);
    let second = exposures.remove(0);
    let (larger, smaller) = if first.size >= second.size {
        (first, second)
    } else {
        (second, first)
    };

    let larger_avg_price = if larger.size > Decimal::ZERO {
        larger.cost / larger.size
    } else {
        Decimal::ZERO
    };
    let smaller_avg_price = if smaller.size > Decimal::ZERO {
        smaller.cost / smaller.size
    } else {
        Decimal::ZERO
    };

    Some(format_directional_summary(
        &html_escape(&larger.outcome),
        larger.size,
        larger_avg_price,
        &html_escape(&smaller.outcome),
        smaller.size,
        smaller_avg_price,
    ))
}

pub(crate) async fn handle_show_positions(
    bot: &Bot,
    chat_id: ChatId,
    db: &Db,
    user_id: i64,
    cb_id: i64,
    source_message: Option<&teloxide::types::MaybeInaccessibleMessage>,
) -> ResponseResult<()> {
    // callback_data rows are addressed by sequential IDs, so only the user
    // who received the activity notification may use its button.
    let callback_data = match db::get_callback_data(db, cb_id).await {
        Ok(Some(data)) if data.user_id == user_id => Some(data),
        Ok(Some(_)) => {
            bot.send_message(chat_id, "This button does not belong to this chat.")
                .await?;
            return Ok(());
        }
        _ => None,
    };
    let wallet_address = match callback_data.as_ref() {
        Some(data) => data.wallet_address.clone(),
        None => {
            let wallet = source_message
                .and_then(|message| message.regular_message())
                .and_then(|message| message.text())
                .and_then(extract_wallet_address_from_text);
            match wallet {
                Some(wallet) => wallet,
                None => {
                    bot.send_message(
                        chat_id,
                        "Could not load position data. This button may be outdated; try a newer activity message.",
                    )
                    .await?;
                    return Ok(());
                }
            }
        }
    };
    let condition_id = callback_data
        .as_ref()
        .and_then(|data| B256::from_str(&data.condition_id).ok());

    let address = match Address::from_str(&wallet_address) {
        Ok(addr) => addr,
        Err(_) => {
            bot.send_message(chat_id, "Invalid wallet address.").await?;
            return Ok(());
        }
    };

    let client = data_client();
    let request = match condition_id {
        Some(condition_id) => {
            let builder = PositionsRequest::builder()
                .user(address)
                .filter(MarketFilter::markets([condition_id]))
                .limit(POSITIONS_PAGE_LIMIT);
            match builder {
                Ok(builder) => builder.build(),
                Err(_) => {
                    bot.send_message(chat_id, "Could not build positions request.")
                        .await?;
                    return Ok(());
                }
            }
        }
        None => {
            let builder = PositionsRequest::builder()
                .user(address)
                .limit(POSITIONS_PAGE_LIMIT);
            match builder {
                Ok(builder) => builder.build(),
                Err(_) => {
                    bot.send_message(chat_id, "Could not build positions request.")
                        .await?;
                    return Ok(());
                }
            }
        }
    };

    let positions = match client.positions(&request).await {
        Ok(positions) => positions,
        Err(_) => {
            bot.send_message(chat_id, "Could not fetch positions.")
                .await?;
            return Ok(());
        }
    };

    let matching: Vec<_> = match condition_id {
        Some(condition_id) => positions
            .iter()
            .filter(|p| p.condition_id == condition_id)
            .collect(),
        None => positions.iter().collect(),
    };

    if matching.is_empty() {
        let message = if condition_id.is_some() {
            "No open positions for this market."
        } else {
            "No open positions for this wallet."
        };
        bot.send_message(chat_id, message).await?;
        return Ok(());
    }

    let mut lines = if condition_id.is_some() {
        let title = &matching[0].title;
        vec![format!("<b>{}</b>\n", html_escape(title))]
    } else {
        vec![format!(
            "<b>Open Positions</b> for <code>{}</code>\n",
            html_escape(&wallet_address)
        )]
    };
    for pos in &matching {
        let label = if condition_id.is_some() {
            html_escape(&pos.outcome)
        } else {
            format!("{}/{}", html_escape(&pos.title), html_escape(&pos.outcome))
        };
        lines.push(format_position_message(
            &label,
            pos.size,
            pos.avg_price,
            pos.cur_price,
            pos.cash_pnl,
        ));
    }

    if condition_id.is_some() {
        if let Some(summary) = build_directional_summary(&matching) {
            lines.push(summary);
        }
    }

    bot.send_message(chat_id, lines.join("\n\n"))
        .parse_mode(ParseMode::Html)
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wallet_type_error_detection_matches_keywords() {
        assert!(is_wallet_type_error("signature type mismatch"));
        assert!(is_wallet_type_error("Proxy wallet derivation"));
        assert!(is_wallet_type_error("Cannot have a funder address"));
        assert!(is_wallet_type_error("USER TYPE invalid"));
        assert!(!is_wallet_type_error("network timeout"));
    }

    #[test]
    fn format_signed_usd_includes_sign() {
        assert_eq!(
            format_signed_usd(Decimal::from_str("120.732").unwrap()),
            "+$120.732"
        );
        assert_eq!(
            format_signed_usd(Decimal::from_str("-72.012").unwrap()),
            "-$72.012"
        );
    }

    #[test]
    fn format_value_change_includes_percent_with_two_decimals() {
        let pnl = Decimal::from_str("120.732").unwrap();
        let cost = Decimal::from_str("917.568").unwrap();
        assert_eq!(format_value_change(pnl, cost), "(+$120.732, +13.16%)");
    }

    #[test]
    fn format_value_change_zero_cost_shows_na_percent() {
        let pnl = Decimal::from_str("10").unwrap();
        assert_eq!(format_value_change(pnl, Decimal::ZERO), "(+$10.000, N/A)");
    }

    #[test]
    fn format_position_message_uses_multiline_layout() {
        let formatted = format_position_message(
            "YES",
            Decimal::from_str("75.758").unwrap(),
            Decimal::from_str("0.660").unwrap(),
            Decimal::from_str("0.855").unwrap(),
            Decimal::from_str("14.773").unwrap(),
        );

        assert_eq!(
            formatted,
            "• <b>YES</b>\n<b>Size:</b> 75.758\n<b>Price:</b> $0.660 (1.52) → $0.855 (1.17)\n<b>Value:</b> $50.000 → $64.773 (+$14.773, +29.55%)"
        );
    }

    #[test]
    fn format_directional_summary_handles_hedged_market_with_net_side() {
        let formatted = format_directional_summary(
            "Natus Vincere",
            Decimal::from_str("4996.590").unwrap(),
            Decimal::from_str("0.398").unwrap(),
            "BetBoom Team",
            Decimal::from_str("4634.810").unwrap(),
            Decimal::from_str("0.607").unwrap(),
        );

        assert_eq!(
            formatted,
            "<b>Directional Summary</b>\n<b>Direction:</b> Natus Vincere +361.780\n<b>Hedged:</b> 4634.810\n<b>Hedge Carry:</b> -$23.174\n<b>If Natus Vincere wins:</b> +$194.618\n<b>If BetBoom Team wins:</b> -$167.162"
        );
    }

    #[test]
    fn build_directional_summary_requires_two_distinct_outcomes() {
        let positions: Vec<&Position> = Vec::new();
        assert!(build_directional_summary(&positions).is_none());
    }
}

pub(crate) async fn handle_auth_key_input(
    bot: &Bot,
    msg: &Message,
    db: &Db,
    user_id: i64,
    data: Option<&str>,
    input: &str,
    encryption_key: Option<EncryptionKey>,
) -> ResponseResult<()> {
    // The key transited Telegram in plaintext. If we cannot remove
    // that message, refuse to store the key and tell the user to
    // consider it exposed.
    if bot.delete_message(msg.chat.id, msg.id).await.is_err() {
        log_db_error(
            db::clear_pending_state(db, user_id).await,
            "clear_pending_state",
            user_id,
        );
        bot.send_message(
            msg.chat.id,
            "⚠️ I couldn't delete your message, so I did <b>not</b> save this key.\n\
             The key was never stored, but it remains visible in this chat — \
             treat it as exposed and move funds to a new wallet.",
        )
        .parse_mode(ParseMode::Html)
        .await?;
        send_manage_menu(&bot, msg.chat.id).await?;
        return Ok(());
    }

    let Some(encryption_key) = encryption_key else {
        log_db_error(
            db::clear_pending_state(db, user_id).await,
            "clear_pending_state",
            user_id,
        );
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
    let (encrypted_key, nonce) =
        match crypto::encrypt(&encryption_key, private_key.as_bytes(), &aad) {
            Ok(payload) => payload,
            Err(_err) => {
                bot.send_message(
                    msg.chat.id,
                    "Sorry, I couldn't secure that key. Try again soon.",
                )
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
        bot.send_message(
            msg.chat.id,
            "Sorry, I couldn't store that wallet. Try again soon.",
        )
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
        bot.send_message(msg.chat.id, format!("Wallet updated to {wallet_address}."))
            .await?;
    }
    bot.send_message(msg.chat.id, MSG_SEND_LABEL_SKIP)
        .reply_markup(manage_label_menu_markup())
        .await?;
    Ok(())
}

pub(crate) async fn handle_auth_label_input(
    bot: &Bot,
    msg: &Message,
    db: &Db,
    user_id: i64,
    data: Option<&str>,
    input: &str,
) -> ResponseResult<()> {
    let Some(wallet_address) = data else {
        log_db_error(
            db::clear_pending_state(db, user_id).await,
            "clear_pending_state",
            user_id,
        );
        bot.send_message(msg.chat.id, MSG_ACTION_EXPIRED).await?;
        return Ok(());
    };

    let label = input.trim();
    if label.is_empty() {
        bot.send_message(msg.chat.id, MSG_SEND_LABEL_SKIP)
            .reply_markup(manage_label_menu_markup())
            .await?;
        return Ok(());
    }

    finalize_manage_label(&bot, msg.chat.id, db, user_id, wallet_address, Some(label)).await?;
    Ok(())
}

pub(crate) async fn handle_positions_input(
    bot: &Bot,
    msg: &Message,
    db: &Db,
    user_id: i64,
) -> ResponseResult<()> {
    log_db_error(
        db::clear_pending_state(db, user_id).await,
        "clear_pending_state",
        user_id,
    );
    send_managed_positions(&bot, msg.chat.id, db, user_id).await?;
    Ok(())
}
