//! Shared pending-action keys, reply strings, and tiny helpers for bot modules.

use teloxide::prelude::*;

pub(crate) const ACTION_TRACK_ADD_ADDRESS: &str = "track_add_address";
pub(crate) const ACTION_TRACK_ADD_LABEL: &str = "track_add_label";
pub(crate) const ACTION_TRACK_REMOVE: &str = "track_remove";
pub(crate) const ACTION_MANAGE_AUTH_KEY: &str = "manage_auth_key";
pub(crate) const ACTION_MANAGE_AUTH_LABEL: &str = "manage_auth_label";
pub(crate) const ACTION_MANAGE_POSITIONS: &str = "manage_positions";
pub(crate) const ACTION_MANAGE_MARKET_ORDER: &str = "manage_market_order";
pub(crate) const ACTION_MANAGE_LIMIT_ORDER: &str = "manage_limit_order";
pub(crate) const ACTION_MANAGE_CANCEL_ORDER: &str = "manage_cancel_order";
pub(crate) const ACTION_COPY_TRADE_EDIT_PRICE: &str = "copy_trade_edit_price";
pub(crate) const ACTION_COPY_TRADE_EDIT_SIZE: &str = "copy_trade_edit_size";

pub(crate) const MSG_ACTION_EXPIRED: &str = "That action expired. Use /start to open the menu.";
pub(crate) const MSG_SEND_LABEL_SKIP: &str = "Send a label for this wallet, or tap Skip.";
pub(crate) const MSG_WALLET_LOAD_FAILED: &str = "Sorry, I couldn't load your managed wallet.";

// "0x" prefix plus 40 hex characters.
pub(crate) const EVM_ADDRESS_LEN: usize = 42;
// Pending-state encoding of SignatureType: Eoa is the default.
pub(crate) const SIG_EOA: &str = "sig:0";
pub(crate) const SIG_PROXY: &str = "sig:1";
pub(crate) const SIG_SAFE: &str = "sig:2";
// Cap positions shown to keep Telegram messages under the length limit.
pub(crate) const POSITIONS_DISPLAY_LIMIT: usize = 8;
pub(crate) const POSITIONS_PAGE_LIMIT: i32 = 200;

pub(crate) fn callback_chat_id(query: &CallbackQuery) -> ChatId {
    query
        .message
        .as_ref()
        .map(|message| message.chat().id)
        .unwrap_or(ChatId(query.from.id.0 as i64))
}

/// Best-effort DB write: failures are logged with context instead of dropped.
pub(crate) fn log_db_error<T>(result: color_eyre::eyre::Result<T>, op: &'static str, user_id: i64) {
    if let Err(err) = result {
        tracing::warn!(user_id, op, error = %err, "db write failed");
    }
}
