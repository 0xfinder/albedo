//! Row-mapping structs for tables also covered by [`crate::db`] helpers.

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, FromRow, Serialize, Deserialize)]
pub struct TrackedWallet {
    pub id: i64,
    pub user_id: i64,
    pub wallet_address: String,
    pub label: Option<String>,
    pub last_activity_hash: Option<String>,
    pub last_positions_hash: Option<String>,
    pub created_at: String,
}

#[derive(Debug, FromRow, Serialize, Deserialize)]
pub struct ManagedWallet {
    pub id: i64,
    pub user_id: i64,
    pub wallet_address: String,
    pub label: Option<String>,
    pub signature_type: i64,
    pub encrypted_key: Vec<u8>,
    pub nonce: Vec<u8>,
    pub created_at: String,
}
