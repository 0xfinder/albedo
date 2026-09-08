//! Inline keyboards and menu-sending helpers.

use crate::db::WalletMode;

use teloxide::payloads::SendMessageSetters;
use teloxide::prelude::*;

use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};

pub(crate) const HELP_TEXT: &str = "Available commands:
\
/start - Start the bot
\
/help - Show this help message
\
/archive - Save wallets without monitoring\n\
/track - Open the track menu
\
/manage - Open the manage menu
\
/version - Show the bot version";

pub(crate) fn main_menu_markup() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![
        InlineKeyboardButton::callback("🧭 Track", "menu:track"),
        InlineKeyboardButton::callback("📦 Archive", "menu:archive"),
        InlineKeyboardButton::callback("⚙️ Manage", "menu:manage"),
    ]])
}

pub(crate) fn track_menu_markup() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![
            InlineKeyboardButton::callback("➕ Add address", "track:add"),
            InlineKeyboardButton::callback("➖ Remove address", "track:remove"),
        ],
        vec![InlineKeyboardButton::callback(
            "📋 View all / Archive wallets",
            "track:list",
        )],
        vec![InlineKeyboardButton::callback("📦 Archive", "menu:archive")],
        vec![InlineKeyboardButton::callback("↩️ Back", "menu:main")],
    ])
}

pub(crate) const ARCHIVE_MENU_TEXT: &str = "Archived wallets are saved but not monitored. Add an address, or view your list to start tracking a wallet.";

pub(crate) fn archive_menu_markup() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![
            InlineKeyboardButton::callback("➕ Archive address", "archive:add"),
            InlineKeyboardButton::callback("🗑️ Delete address", "archive:remove"),
        ],
        vec![InlineKeyboardButton::callback(
            "📋 View archive",
            "archive:list",
        )],
        vec![InlineKeyboardButton::callback(
            "🧭 Tracked wallets",
            "menu:track",
        )],
        vec![InlineKeyboardButton::callback("↩️ Back", "menu:main")],
    ])
}

pub(crate) async fn send_wallet_menu(
    bot: &Bot,
    chat_id: ChatId,
    mode: WalletMode,
) -> ResponseResult<()> {
    match mode {
        WalletMode::Tracked => send_track_menu(bot, chat_id).await,
        WalletMode::Archived => {
            bot.send_message(chat_id, ARCHIVE_MENU_TEXT)
                .reply_markup(archive_menu_markup())
                .await?;
            Ok(())
        }
    }
}

pub(crate) fn wallet_cancel_markup(mode: WalletMode) -> InlineKeyboardMarkup {
    let callback = if mode.is_archived() {
        "archive:cancel"
    } else {
        "action:cancel"
    };
    InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
        "Cancel", callback,
    )]])
}

pub(crate) fn label_menu_markup(mode: WalletMode) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::callback(
            "Skip label",
            if mode.is_archived() {
                "archive:skip_label"
            } else {
                "track:skip_label"
            },
        )],
        vec![InlineKeyboardButton::callback(
            "Cancel",
            if mode.is_archived() {
                "archive:cancel"
            } else {
                "action:cancel"
            },
        )],
    ])
}

pub(crate) fn manage_menu_markup() -> InlineKeyboardMarkup {
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
        vec![InlineKeyboardButton::callback(
            "🗑️ Remove wallet",
            "manage:remove",
        )],
        vec![InlineKeyboardButton::callback("↩️ Back", "menu:main")],
    ])
}

pub(crate) fn manage_label_menu_markup() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::callback(
            "Skip label",
            "manage:skip_label",
        )],
        vec![InlineKeyboardButton::callback(
            "Cancel",
            "manage:cancel_action",
        )],
    ])
}

pub(crate) fn manage_cancel_menu_markup() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
        "Cancel",
        "manage:cancel_action",
    )]])
}

pub(crate) fn manage_wallet_type_setup_markup() -> InlineKeyboardMarkup {
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

pub(crate) fn manage_wallet_type_change_markup() -> InlineKeyboardMarkup {
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

pub(crate) fn manage_wallet_type_prompt_markup() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
        "Change wallet type",
        "manage:wallet_type",
    )]])
}

pub(crate) fn manage_remove_confirm_markup() -> InlineKeyboardMarkup {
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

pub(crate) async fn send_callback_menu(
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

pub(crate) async fn send_track_menu(bot: &Bot, chat_id: ChatId) -> ResponseResult<()> {
    bot.send_message(chat_id, "Track wallets: add, remove, or review your list.")
        .reply_markup(track_menu_markup())
        .await?;
    Ok(())
}
