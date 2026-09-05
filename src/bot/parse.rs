//! Input parsing and display formatting shared by bot modules.

use std::str::FromStr;

use polymarket_client_sdk::clob::types::{Side, SignatureType};
use polymarket_client_sdk::types::{Decimal, U256};
use teloxide::utils::command::parse_command;

use super::common::{EVM_ADDRESS_LEN, SIG_PROXY, SIG_SAFE};
use crate::utils::number_format;

pub(crate) fn normalize_wallet_address(raw: &str) -> String {
    raw.trim().to_lowercase()
}

pub(crate) fn is_valid_wallet_address(raw: &str) -> bool {
    let trimmed = raw.trim();
    if !trimmed.starts_with("0x") {
        return false;
    }

    if trimmed.len() != EVM_ADDRESS_LEN {
        return false;
    }

    trimmed[2..].chars().all(|c| c.is_ascii_hexdigit())
}

pub(crate) fn parse_incoming_command(text: &str, bot_name: &str) -> Option<(String, Vec<String>)> {
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
        "start" | "help" | "track" | "manage" | "version" => Some((command, args)),
        _ => None,
    }
}

pub(crate) fn parse_signature_type(data: Option<&str>) -> SignatureType {
    match data {
        Some(SIG_PROXY) => SignatureType::Proxy,
        Some(SIG_SAFE) => SignatureType::GnosisSafe,
        _ => SignatureType::Eoa,
    }
}

pub(crate) fn signature_type_from_db(raw: i64) -> SignatureType {
    match raw {
        1 => SignatureType::Proxy,
        2 => SignatureType::GnosisSafe,
        _ => SignatureType::Eoa,
    }
}

pub(crate) fn format_signature_type(signature_type: SignatureType) -> &'static str {
    match signature_type {
        SignatureType::Proxy => "Email/Google login (Magic)",
        SignatureType::GnosisSafe => "Gnosis Safe",
        SignatureType::Eoa => "Standard wallet (MetaMask/Ledger)",
        _ => "Standard wallet (MetaMask/Ledger)",
    }
}

pub(crate) fn parse_side(raw: &str) -> Option<Side> {
    match raw.trim().to_lowercase().as_str() {
        "buy" => Some(Side::Buy),
        "sell" => Some(Side::Sell),
        _ => None,
    }
}

pub(crate) fn parse_token_id(raw: &str) -> Option<U256> {
    U256::from_str(raw.trim()).ok()
}

pub(crate) fn parse_decimal(raw: &str) -> Option<Decimal> {
    Decimal::from_str(raw.trim()).ok()
}

pub(crate) fn format_decimal(value: Decimal) -> String {
    number_format::format_value(value)
}

pub(crate) fn format_signed_usd(value: Decimal) -> String {
    let sign = if value < Decimal::ZERO { "-" } else { "+" };
    let magnitude = if value < Decimal::ZERO { -value } else { value };
    format!("{sign}{}", number_format::format_usd(magnitude))
}

pub(crate) fn format_signed_percent(value: Decimal) -> String {
    format!("{:+.2}%", value.round_dp(2))
}

pub(crate) fn format_value_change(pnl: Decimal, cost: Decimal) -> String {
    let pnl_display = format_signed_usd(pnl);
    if cost > Decimal::ZERO {
        let pnl_percent = (pnl / cost) * Decimal::from(100);
        format!("({pnl_display}, {})", format_signed_percent(pnl_percent))
    } else {
        format!("({pnl_display}, N/A)")
    }
}

pub(crate) fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub(crate) fn extract_wallet_address_from_text(text: &str) -> Option<String> {
    text.split(|c: char| !(c.is_ascii_hexdigit() || c == 'x' || c == 'X'))
        .find(|part| {
            part.len() == EVM_ADDRESS_LEN
                && (part.starts_with("0x") || part.starts_with("0X"))
                && part[2..].chars().all(|c| c.is_ascii_hexdigit())
        })
        .map(|part| part.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bot::common::{SIG_EOA, SIG_PROXY};

    #[test]
    fn parse_signature_type_defaults_to_eoa() {
        assert_eq!(parse_signature_type(None), SignatureType::Eoa);
        assert_eq!(parse_signature_type(Some("unknown")), SignatureType::Eoa);
    }

    #[test]
    fn parse_signature_type_maps_values() {
        assert_eq!(parse_signature_type(Some(SIG_EOA)), SignatureType::Eoa);
        assert_eq!(parse_signature_type(Some(SIG_PROXY)), SignatureType::Proxy);
        assert_eq!(
            parse_signature_type(Some("sig:2")),
            SignatureType::GnosisSafe
        );
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
        assert_eq!(
            format_signature_type(SignatureType::Eoa),
            "Standard wallet (MetaMask/Ledger)"
        );
        assert_eq!(
            format_signature_type(SignatureType::Proxy),
            "Email/Google login (Magic)"
        );
        assert_eq!(
            format_signature_type(SignatureType::GnosisSafe),
            "Gnosis Safe"
        );
    }
}
