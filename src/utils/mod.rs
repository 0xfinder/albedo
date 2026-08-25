pub mod crypto;
pub mod number_format;

/// An empty allowlist locks the bot down; only listed IDs are allowed.
pub fn telegram_id_allowed(allowed_ids: &[i64], telegram_id: i64) -> bool {
    allowed_ids.contains(&telegram_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_allowlist_allows_nobody() {
        assert!(!telegram_id_allowed(&[], 123));
    }

    #[test]
    fn listed_id_is_allowed() {
        assert!(telegram_id_allowed(&[123, 456], 123));
        assert!(!telegram_id_allowed(&[123, 456], 789));
    }
}
