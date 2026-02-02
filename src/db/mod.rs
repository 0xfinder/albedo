pub mod models;

use color_eyre::eyre::Result;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::SqlitePool;
use std::str::FromStr;

pub type Db = SqlitePool;

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

pub async fn set_pending_action(db: &Db, user_id: i64, action: &str) -> Result<()> {
    sqlx::query("UPDATE users SET pending_action = ? WHERE id = ?")
        .bind(action)
        .bind(user_id)
        .execute(db)
        .await?;

    Ok(())
}

pub async fn clear_pending_action(db: &Db, user_id: i64) -> Result<()> {
    sqlx::query("UPDATE users SET pending_action = NULL WHERE id = ?")
        .bind(user_id)
        .execute(db)
        .await?;

    Ok(())
}

pub async fn get_pending_action(db: &Db, user_id: i64) -> Result<Option<String>> {
    let (action,) = sqlx::query_as::<_, (Option<String>,)>(
        "SELECT pending_action FROM users WHERE id = ?",
    )
    .bind(user_id)
    .fetch_one(db)
    .await?;

    Ok(action)
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
        "SELECT id, user_id, wallet_address, label, created_at FROM tracked_wallets WHERE user_id = ? ORDER BY created_at",
    )
    .bind(user_id)
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
