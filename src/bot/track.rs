//! Tracked and archived wallet menus share address and label input.

use teloxide::payloads::SendMessageSetters;
use teloxide::prelude::*;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, ParseMode};

use super::common::{
    ACTION_ARCHIVE_ADD_LABEL, ACTION_TRACK_ADD_LABEL, MSG_ACTION_EXPIRED, MSG_SEND_LABEL_SKIP,
    log_db_error,
};
use super::menus::{label_menu_markup, send_wallet_menu};
use super::parse::{WalletAddress, html_escape};
use crate::db::{self, Db, WalletMode};

pub(crate) async fn finalize_wallet_add(
    bot: &Bot,
    chat_id: ChatId,
    db: &Db,
    user_id: i64,
    wallet_address: &str,
    label: Option<&str>,
    mode: WalletMode,
) -> ResponseResult<()> {
    let inserted = match db::save_wallet(db, user_id, wallet_address, label, mode).await {
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

    let response = if inserted {
        if mode.is_archived() {
            format!("Archived {wallet_address}. Saved without monitoring.")
        } else {
            format!("Now tracking {wallet_address}.")
        }
    } else if mode.is_archived() {
        "That wallet is already archived.".to_string()
    } else {
        "That wallet is already being tracked.".to_string()
    };
    bot.send_message(chat_id, response).await?;
    send_wallet_menu(bot, chat_id, mode).await?;
    Ok(())
}

pub(crate) async fn send_wallets(
    bot: &Bot,
    chat_id: ChatId,
    db: &Db,
    user_id: i64,
    mode: WalletMode,
) -> ResponseResult<()> {
    let wallets = match db::list_wallets(db, user_id, mode).await {
        Ok(wallets) => wallets,
        Err(_err) => {
            bot.send_message(chat_id, "Sorry, I couldn't load your wallets.")
                .await?;
            return Ok(());
        }
    };

    if wallets.is_empty() {
        let message = if mode.is_archived() {
            "No archived wallets yet. Use Archive address to save one without monitoring."
        } else {
            "No tracked wallets yet. Use Add address to start monitoring."
        };
        bot.send_message(chat_id, message).await?;
    } else {
        let title = if mode.is_archived() {
            "Archived. Not monitored"
        } else {
            "Tracking"
        };
        bot.send_message(chat_id, format!("{title}: {} wallet(s)", wallets.len()))
            .await?;
        let (action, callback) = if mode.is_archived() {
            ("Start tracking", "wallet:track")
        } else {
            ("Archive", "wallet:archive")
        };
        for chunk in wallets.chunks(8) {
            let mut lines = Vec::new();
            let mut buttons = Vec::new();
            for (index, wallet) in chunk.iter().enumerate() {
                let number = index + 1;
                let label: String = wallet
                    .label
                    .as_deref()
                    .unwrap_or("Unlabeled")
                    .chars()
                    .take(80)
                    .collect();
                lines.push(format!(
                    "{number}. <b>{}</b> / <a href=\"https://polymarket.com/profile/{}\">profile</a>\n<code>{}</code>",
                    html_escape(&label), wallet.wallet_address, wallet.wallet_address
                ));
                buttons.push(vec![InlineKeyboardButton::callback(
                    format!("{action} {number}"),
                    format!("{callback}:{}", wallet.id),
                )]);
            }
            bot.send_message(chat_id, lines.join("\n\n"))
                .parse_mode(ParseMode::Html)
                .link_preview_options(teloxide::types::LinkPreviewOptions {
                    is_disabled: true,
                    url: None,
                    prefer_small_media: false,
                    prefer_large_media: false,
                    show_above_text: false,
                })
                .reply_markup(InlineKeyboardMarkup::new(buttons))
                .await?;
        }
    }
    send_wallet_menu(bot, chat_id, mode).await?;
    Ok(())
}

pub(crate) async fn handle_address_input(
    bot: &Bot,
    msg: &Message,
    db: &Db,
    user_id: i64,
    input: &str,
    mode: WalletMode,
) -> ResponseResult<()> {
    let Some(wallet_address) = WalletAddress::parse(input) else {
        bot.send_message(
            msg.chat.id,
            "That wallet address looks invalid. Expected 0x + 40 hex characters.",
        )
        .await?;
        return Ok(());
    };
    let wallet_address = wallet_address.as_str();
    if let Err(_err) = db::set_pending_state(
        db,
        user_id,
        Some(if mode.is_archived() {
            ACTION_ARCHIVE_ADD_LABEL
        } else {
            ACTION_TRACK_ADD_LABEL
        }),
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
        .reply_markup(label_menu_markup(mode))
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
    mode: WalletMode,
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
            .reply_markup(label_menu_markup(mode))
            .await?;
        return Ok(());
    }

    finalize_wallet_add(
        &bot,
        msg.chat.id,
        db,
        user_id,
        wallet_address,
        Some(label),
        mode,
    )
    .await?;
    Ok(())
}

pub(crate) async fn handle_remove_input(
    bot: &Bot,
    msg: &Message,
    db: &Db,
    user_id: i64,
    input: &str,
    mode: WalletMode,
) -> ResponseResult<()> {
    let Some(wallet_address) = WalletAddress::parse(input) else {
        bot.send_message(
            msg.chat.id,
            "That wallet address looks invalid. Expected 0x + 40 hex characters.",
        )
        .await?;
        return Ok(());
    };
    let wallet_address = wallet_address.as_str();
    let removed = match db::remove_wallet(db, user_id, &wallet_address, mode).await {
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
        bot.send_message(
            msg.chat.id,
            format!(
                "Removed {wallet_address} from your {} wallets.",
                if mode.is_archived() {
                    "archived"
                } else {
                    "tracked"
                }
            ),
        )
        .await?;
    } else {
        bot.send_message(msg.chat.id, "That wallet is not in this list.")
            .await?;
    }

    send_wallet_menu(&bot, msg.chat.id, mode).await?;
    Ok(())
}
