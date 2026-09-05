pub mod crypto;
pub mod number_format;

use std::collections::HashSet;

/// Telegram user IDs allowed to interact with the bot. An empty allowlist
/// locks the bot down; only listed IDs are allowed.
#[derive(Debug, Clone, Default)]
pub struct Allowlist(HashSet<i64>);

impl Allowlist {
    pub fn from_ids(ids: impl IntoIterator<Item = i64>) -> Self {
        Self(ids.into_iter().collect())
    }

    pub fn is_allowed(&self, telegram_id: i64) -> bool {
        self.0.contains(&telegram_id)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_allowlist_allows_nobody() {
        assert!(!Allowlist::default().is_allowed(123));
    }

    #[test]
    fn listed_id_is_allowed() {
        let allowlist = Allowlist::from_ids([123, 456]);
        assert!(allowlist.is_allowed(123));
        assert!(!allowlist.is_allowed(789));
    }
}
