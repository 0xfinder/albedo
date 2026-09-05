//! Tracked-wallet menus: add, list, and finalize.

use teloxide::payloads::SendMessageSetters;
use teloxide::prelude::*;
use teloxide::types::ParseMode;

use super::common::log_db_error;
use super::menus::send_track_menu;
use super::parse::html_escape;
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
