use polymarket_client_sdk::types::Decimal;
use std::str::FromStr;

#[derive(Debug, Clone, Copy)]
pub struct NumberFormatConfig {
    pub value_decimals: u32,
    pub odds_decimals: u32,
}

pub const NUMBER_FORMAT_CONFIG: NumberFormatConfig = NumberFormatConfig {
    value_decimals: 3,
    odds_decimals: 2,
};

fn format_with_decimals(value: Decimal, decimals: u32) -> String {
    format!("{:.*}", decimals as usize, value.round_dp(decimals))
}

pub fn format_value(value: Decimal) -> String {
    format_with_decimals(value, NUMBER_FORMAT_CONFIG.value_decimals)
}

pub fn format_option_value(value: Option<Decimal>) -> String {
    value.map(format_value).unwrap_or_else(|| "N/A".to_string())
}

pub fn format_usd(value: Decimal) -> String {
    format!("${}", format_value(value))
}

pub fn decimal_odds_from_price(price: Decimal) -> Option<Decimal> {
    if price > Decimal::ZERO {
        Some(Decimal::ONE / price)
    } else {
        None
    }
}

pub fn format_price_with_odds(price: Decimal) -> String {
    let usd = format_usd(price);
    match decimal_odds_from_price(price) {
        Some(odds) => format!(
            "{usd} ({})",
            format_with_decimals(odds, NUMBER_FORMAT_CONFIG.odds_decimals)
        ),
        None => format!("{usd} (N/A)"),
    }
}

pub fn format_price_with_odds_str(raw: &str) -> Option<String> {
    Decimal::from_str(raw.trim())
        .ok()
        .map(format_price_with_odds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_value_uses_fixed_decimals() {
        let value = Decimal::from_str("1.5").unwrap();
        assert_eq!(format_value(value), "1.500");
    }

    #[test]
    fn format_price_with_odds_formats_both_values() {
        let price = Decimal::from_str("0.5").unwrap();
        assert_eq!(format_price_with_odds(price), "$0.500 (2.00)");
    }

    #[test]
    fn format_price_with_odds_handles_zero() {
        let price = Decimal::ZERO;
        assert_eq!(format_price_with_odds(price), "$0.000 (N/A)");
    }
}
