//! Tracked-wallet menus: add, list, and finalize.

use teloxide::payloads::SendMessageSetters;
use teloxide::prelude::*;
use teloxide::types::ParseMode;

use super::common::{
    ACTION_TRACK_ADD_LABEL, MSG_ACTION_EXPIRED, MSG_SEND_LABEL_SKIP, log_db_error,
};
use super::menus::{label_menu_markup, send_track_menu};
use super::parse::{html_escape, is_valid_wallet_address, normalize_wallet_address};
use crate::db::{self, Db};

pub(crate) async fn finalize_track_add(
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
            bot.send_message(
                chat_id,
                "Sorry, I couldn't add that wallet. Try again soon.",
            )
            .await?;
            return Ok(());
        }
    };

    log_db_error(
        db::clear_pending_state(db, user_id).await,
        "clear_pending_state",
        user_id,
    );

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

pub(crate) async fn send_tracked_wallets(
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
            "<b>{}</b> / <a href=\"{}\">profile</a>\nWallet: <code>{}</code>",
            html_escape(label_text),
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
        .parse_mode(ParseMode::Html)
        .link_preview_options(teloxide::types::LinkPreviewOptions {
            is_disabled: true,
            url: None,
            prefer_small_media: false,
            prefer_large_media: false,
            show_above_text: false,
        })
        .await?;
    Ok(())
}

pub(crate) async fn handle_address_input(
    bot: &Bot,
    msg: &Message,
    db: &Db,
    user_id: i64,
    input: &str,
) -> ResponseResult<()> {
    if !is_valid_wallet_address(input) {
        bot.send_message(
            msg.chat.id,
            "That wallet address looks invalid. Expected 0x + 40 hex characters.",
        )
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
        bot.send_message(
            msg.chat.id,
            "Sorry, I couldn't continue that request. Try again soon.",
        )
        .await?;
        return Ok(());
    }
    bot.send_message(msg.chat.id, MSG_SEND_LABEL_SKIP)
        .reply_markup(label_menu_markup())
        .await?;
    Ok(())
}

pub(crate) async fn handle_label_input(
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
            .reply_markup(label_menu_markup())
            .await?;
        return Ok(());
    }

    finalize_track_add(&bot, msg.chat.id, db, user_id, wallet_address, Some(label)).await?;
    Ok(())
}

pub(crate) async fn handle_remove_input(
    bot: &Bot,
    msg: &Message,
    db: &Db,
    user_id: i64,
    input: &str,
) -> ResponseResult<()> {
    if !is_valid_wallet_address(input) {
        bot.send_message(
            msg.chat.id,
            "That wallet address looks invalid. Expected 0x + 40 hex characters.",
        )
        .await?;
        return Ok(());
    }

    let wallet_address = normalize_wallet_address(input);
    let removed = match db::remove_tracked_wallet(db, user_id, &wallet_address).await {
        Ok(removed) => removed,
        Err(_err) => {
            bot.send_message(
                msg.chat.id,
                "Sorry, I couldn't remove that wallet. Try again soon.",
            )
            .await?;
            return Ok(());
        }
    };

    log_db_error(
        db::clear_pending_state(db, user_id).await,
        "clear_pending_state",
        user_id,
    );

    if removed {
        bot.send_message(msg.chat.id, format!("Stopped tracking {wallet_address}."))
            .await?;
    } else {
        bot.send_message(msg.chat.id, "That wallet is not being tracked.")
            .await?;
    }

    send_track_menu(&bot, msg.chat.id).await?;
    Ok(())
}
