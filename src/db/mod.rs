pub mod models;

use color_eyre::eyre::Result;
use sqlx::SqlitePool;

pub type Db = SqlitePool;

pub async fn init(database_url: &str) -> Result<Db> {
    let pool = SqlitePool::connect(database_url).await?;

    sqlx::query("PRAGMA foreign_keys = ON;").execute(&pool).await?;
    
    // Run migrations
    sqlx::migrate!("./src/db/migrations").run(&pool).await?;
    
    Ok(pool)
}

pub async fn upsert_user(db: &Db, telegram_id: i64, chat_id: i64) -> Result<()> {
    sqlx::query(
        "INSERT INTO users (telegram_id, chat_id, current_mode) VALUES (?, ?, 'none')\
         ON CONFLICT(telegram_id) DO UPDATE SET chat_id = excluded.chat_id, last_active = CURRENT_TIMESTAMP",
    )
    .bind(telegram_id)
    .bind(chat_id)
    .execute(db)
    .await?;

    Ok(())
}

pub async fn set_mode(db: &Db, telegram_id: i64, mode: &str) -> Result<()> {
    sqlx::query(
        "UPDATE users SET current_mode = ?, last_active = CURRENT_TIMESTAMP WHERE telegram_id = ?",
    )
    .bind(mode)
    .bind(telegram_id)
    .execute(db)
    .await?;

    Ok(())
}
