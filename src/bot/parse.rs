//! Input parsing and display formatting shared by bot modules.

use std::str::FromStr;

use polymarket_client_sdk::clob::types::{Side, SignatureType};
use polymarket_client_sdk::types::{Decimal, U256};
use teloxide::utils::command::parse_command;

use super::common::{EVM_ADDRESS_LEN, SIG_PROXY, SIG_SAFE};
use crate::utils::number_format;

/// Lowercase `0x` + 40 hex wallet address. The only way to build one from
/// user input is [`WalletAddress::parse`], so validated addresses cannot be
/// confused with raw strings.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct WalletAddress(String);

impl WalletAddress {
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        let normalized = raw.trim().to_lowercase();
        if !normalized.starts_with("0x") {
            return None;
        }
        if normalized.len() != EVM_ADDRESS_LEN {
            return None;
        }
        if !normalized[2..].chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        Some(Self(normalized))
    }

    /// Find the first valid address in free text, e.g. a pasted message.
    pub(crate) fn extract(text: &str) -> Option<Self> {
        text.split(|c: char| !(c.is_ascii_hexdigit() || c == 'x' || c == 'X'))
            .find_map(Self::parse)
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for WalletAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
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

    #[test]
    fn wallet_address_parse_accepts_normalized_forms() {
        let lower = "0x".to_string() + &"ab".repeat(20);
        assert_eq!(
            WalletAddress::parse(&lower)
                .as_ref()
                .map(WalletAddress::as_str),
            Some(lower.as_str())
        );
        assert_eq!(
            WalletAddress::parse(&("  ".to_string() + &lower.to_uppercase() + "  "))
                .as_ref()
                .map(WalletAddress::as_str),
            Some(lower.as_str())
        );
    }

    #[test]
    fn wallet_address_parse_rejects_bad_input() {
        assert!(WalletAddress::parse("").is_none());
        assert!(WalletAddress::parse("0x1234").is_none());
        assert!(WalletAddress::parse(&("0x".to_string() + &"zz".repeat(20))).is_none());
        assert!(WalletAddress::parse("not an address").is_none());
    }

    #[test]
    fn wallet_address_extract_finds_first_address() {
        let address = "0x".to_string() + &"12".repeat(20);
        let found = WalletAddress::extract(&format!("send to {address} please"));
        assert_eq!(
            found.as_ref().map(WalletAddress::as_str),
            Some(address.as_str())
        );
        assert!(WalletAddress::extract("nothing here").is_none());
    }
}
