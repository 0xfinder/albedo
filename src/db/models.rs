use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[allow(dead_code)]
#[derive(Debug, FromRow, Serialize, Deserialize)]
pub struct User {
    pub id: i64,
    pub telegram_id: i64,
    pub chat_id: i64,
    pub current_mode: String,
    pub created_at: String,
    pub last_active: String,
}

#[derive(Debug, FromRow, Serialize, Deserialize)]
pub struct TrackedWallet {
    pub id: i64,
    pub user_id: i64,
    pub wallet_address: String,
    pub label: Option<String>,
    pub created_at: String,
}

#[allow(dead_code)]
#[derive(Debug, FromRow, Serialize, Deserialize)]
pub struct PositionSnapshot {
    pub id: i64,
    pub wallet_address: String,
    pub market_slug: Option<String>,
    pub market_question: Option<String>,
    pub outcome: Option<String>,
    pub position_size: Option<String>,
    pub avg_price: Option<String>,
    pub total_value: Option<String>,
    pub snapshot_time: String,
}

#[allow(dead_code)]
#[derive(Debug, FromRow, Serialize, Deserialize)]
pub struct ManagedWallet {
    pub id: i64,
    pub user_id: i64,
    pub wallet_address: String,
    pub encrypted_key: Vec<u8>,
    pub nonce: Vec<u8>,
    pub created_at: String,
}

#[allow(dead_code)]
#[derive(Debug, FromRow, Serialize, Deserialize)]
pub struct ActivityLog {
    pub id: i64,
    pub wallet_address: String,
    pub activity_type: String,
    pub market_slug: Option<String>,
    pub details: Option<String>,
    pub notified: bool,
    pub created_at: String,
}
