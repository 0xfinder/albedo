pub mod models;

use color_eyre::eyre::Result;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{FromRow, SqlitePool};
use std::str::FromStr;

pub type Db = SqlitePool;

#[derive(Debug, FromRow)]
pub struct TrackedWalletWithUser {
    pub user_id: i64,
    pub chat_id: i64,
    pub wallet_address: String,
    pub label: Option<String>,
    pub last_activity_hash: Option<String>,
    pub last_positions_hash: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, FromRow)]
pub struct ManagedWalletWithUser {
    pub user_id: i64,
    pub chat_id: i64,
    pub wallet_address: String,
    pub label: Option<String>,
    pub encrypted_key: Vec<u8>,
    pub nonce: Vec<u8>,
}

pub async fn init(database_url: &str) -> Result<Db> {
    let options = SqliteConnectOptions::from_str(database_url)?.create_if_missing(true);
    let pool = SqlitePool::connect_with(options).await?;

    sqlx::query("PRAGMA foreign_keys = ON;").execute(&pool).await?;
    
    // Run migrations
    sqlx::migrate!("./src/db/migrations").run(&pool).await?;
    
    Ok(pool)
}

pub async fn ensure_user(db: &Db, telegram_id: i64, chat_id: i64) -> Result<i64> {
    sqlx::query(
        "INSERT INTO users (telegram_id, chat_id, current_mode) VALUES (?, ?, 'none')\
         ON CONFLICT(telegram_id) DO UPDATE SET chat_id = excluded.chat_id, last_active = CURRENT_TIMESTAMP",
    )
    .bind(telegram_id)
    .bind(chat_id)
    .execute(db)
    .await?;

    let (user_id,) = sqlx::query_as::<_, (i64,)>("SELECT id FROM users WHERE telegram_id = ?")
        .bind(telegram_id)
        .fetch_one(db)
        .await?;

    Ok(user_id)
}

pub async fn set_mode(db: &Db, user_id: i64, mode: &str) -> Result<()> {
    sqlx::query("UPDATE users SET current_mode = ?, last_active = CURRENT_TIMESTAMP WHERE id = ?")
    .bind(mode)
    .bind(user_id)
    .execute(db)
    .await?;

    Ok(())
}

pub async fn set_pending_state(
    db: &Db,
    user_id: i64,
    action: Option<&str>,
    data: Option<&str>,
) -> Result<()> {
    sqlx::query("UPDATE users SET pending_action = ?, pending_data = ? WHERE id = ?")
        .bind(action)
        .bind(data)
        .bind(user_id)
        .execute(db)
        .await?;

    Ok(())
}

pub async fn clear_pending_state(db: &Db, user_id: i64) -> Result<()> {
    set_pending_state(db, user_id, None, None).await
}

pub async fn get_pending_state(db: &Db, user_id: i64) -> Result<(Option<String>, Option<String>)> {
    let (action, data) = sqlx::query_as::<_, (Option<String>, Option<String>)>(
        "SELECT pending_action, pending_data FROM users WHERE id = ?",
    )
    .bind(user_id)
    .fetch_one(db)
    .await?;

    Ok((action, data))
}

pub async fn add_tracked_wallet(
    db: &Db,
    user_id: i64,
    wallet_address: &str,
    label: Option<&str>,
) -> Result<bool> {
    let result = sqlx::query(
        "INSERT INTO tracked_wallets (user_id, wallet_address, label) VALUES (?, ?, ?)\
         ON CONFLICT(user_id, wallet_address) DO NOTHING",
    )
    .bind(user_id)
    .bind(wallet_address)
    .bind(label)
    .execute(db)
    .await?;

    Ok(result.rows_affected() > 0)
}

pub async fn list_tracked_wallets(db: &Db, user_id: i64) -> Result<Vec<models::TrackedWallet>> {
    let wallets = sqlx::query_as::<_, models::TrackedWallet>(
        "SELECT id, user_id, wallet_address, label, last_activity_hash, last_positions_hash, created_at FROM tracked_wallets WHERE user_id = ? ORDER BY created_at",
    )
    .bind(user_id)
    .fetch_all(db)
    .await?;

    Ok(wallets)
}

pub async fn upsert_managed_wallet(
    db: &Db,
    user_id: i64,
    wallet_address: &str,
    encrypted_key: &[u8],
    nonce: &[u8],
    label: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO managed_wallets (user_id, wallet_address, encrypted_key, nonce, label) VALUES (?, ?, ?, ?, ?)\
         ON CONFLICT(user_id, wallet_address) DO UPDATE SET encrypted_key = excluded.encrypted_key, nonce = excluded.nonce, label = COALESCE(excluded.label, managed_wallets.label)",
    )
    .bind(user_id)
    .bind(wallet_address)
    .bind(encrypted_key)
    .bind(nonce)
    .bind(label)
    .execute(db)
    .await?;

    Ok(())
}

pub async fn update_managed_wallet_label(
    db: &Db,
    user_id: i64,
    wallet_address: &str,
    label: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "UPDATE managed_wallets SET label = ? WHERE user_id = ? AND wallet_address = ?",
    )
    .bind(label)
    .bind(user_id)
    .bind(wallet_address)
    .execute(db)
    .await?;

    Ok(())
}

pub async fn list_managed_wallets(db: &Db, user_id: i64) -> Result<Vec<models::ManagedWallet>> {
    let wallets = sqlx::query_as::<_, models::ManagedWallet>(
        "SELECT id, user_id, wallet_address, label, encrypted_key, nonce, created_at FROM managed_wallets WHERE user_id = ? ORDER BY created_at",
    )
    .bind(user_id)
    .fetch_all(db)
    .await?;

    Ok(wallets)
}

pub async fn get_managed_wallet(
    db: &Db,
    user_id: i64,
    wallet_address: &str,
) -> Result<Option<models::ManagedWallet>> {
    let wallet = sqlx::query_as::<_, models::ManagedWallet>(
        "SELECT id, user_id, wallet_address, label, encrypted_key, nonce, created_at FROM managed_wallets WHERE user_id = ? AND wallet_address = ?",
    )
    .bind(user_id)
    .bind(wallet_address)
    .fetch_optional(db)
    .await?;

    Ok(wallet)
}

pub async fn list_managed_wallets_with_users(db: &Db) -> Result<Vec<ManagedWalletWithUser>> {
    let wallets = sqlx::query_as::<_, ManagedWalletWithUser>(
        "SELECT managed_wallets.user_id, users.chat_id, managed_wallets.wallet_address, managed_wallets.label, managed_wallets.encrypted_key, managed_wallets.nonce\
         FROM managed_wallets\
         INNER JOIN users ON users.id = managed_wallets.user_id",
    )
    .fetch_all(db)
    .await?;

    Ok(wallets)
}

pub async fn remove_managed_wallet(db: &Db, user_id: i64, wallet_address: &str) -> Result<bool> {
    let result = sqlx::query(
        "DELETE FROM managed_wallets WHERE user_id = ? AND wallet_address = ?",
    )
    .bind(user_id)
    .bind(wallet_address)
    .execute(db)
    .await?;

    Ok(result.rows_affected() > 0)
}

pub async fn list_tracked_wallets_with_users(db: &Db) -> Result<Vec<TrackedWalletWithUser>> {
    let wallets = sqlx::query_as::<_, TrackedWalletWithUser>(
        "SELECT tracked_wallets.user_id, users.chat_id, tracked_wallets.wallet_address, tracked_wallets.label, tracked_wallets.last_activity_hash, tracked_wallets.last_positions_hash\
         FROM tracked_wallets\
         INNER JOIN users ON users.id = tracked_wallets.user_id",
    )
    .fetch_all(db)
    .await?;

    Ok(wallets)
}

pub async fn remove_tracked_wallet(db: &Db, user_id: i64, wallet_address: &str) -> Result<bool> {
    let result = sqlx::query(
        "DELETE FROM tracked_wallets WHERE user_id = ? AND wallet_address = ?",
    )
    .bind(user_id)
    .bind(wallet_address)
    .execute(db)
    .await?;

    Ok(result.rows_affected() > 0)
}

pub async fn count_tracked_wallets(db: &Db, user_id: i64) -> Result<i64> {
    let (count,) =
        sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM tracked_wallets WHERE user_id = ?")
            .bind(user_id)
            .fetch_one(db)
            .await?;

    Ok(count)
}

pub async fn update_tracked_wallet_activity_hash(
    db: &Db,
    user_id: i64,
    wallet_address: &str,
    hash: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "UPDATE tracked_wallets SET last_activity_hash = ? WHERE user_id = ? AND wallet_address = ?",
    )
    .bind(hash)
    .bind(user_id)
    .bind(wallet_address)
    .execute(db)
    .await?;

    Ok(())
}

pub async fn update_tracked_wallet_positions_hash(
    db: &Db,
    user_id: i64,
    wallet_address: &str,
    hash: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "UPDATE tracked_wallets SET last_positions_hash = ? WHERE user_id = ? AND wallet_address = ?",
    )
    .bind(hash)
    .bind(user_id)
    .bind(wallet_address)
    .execute(db)
    .await?;

    Ok(())
}
