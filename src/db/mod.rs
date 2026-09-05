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
    pub telegram_id: i64,
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
    pub telegram_id: i64,
    pub wallet_address: String,
    pub label: Option<String>,
    pub encrypted_key: Vec<u8>,
    pub nonce: Vec<u8>,
}

#[derive(Debug, FromRow)]
pub struct CallbackData {
    pub user_id: i64,
    pub wallet_address: String,
    pub condition_id: String,
    pub token_id: Option<String>,
    pub side: Option<String>,
    pub price: Option<String>,
    pub size: Option<String>,
    pub market_title: Option<String>,
    pub outcome: Option<String>,
}

#[derive(Debug, FromRow)]
pub struct CopyTradeState {
    pub id: i64,
    pub user_id: i64,
    pub token_id: String,
    pub side: String,
    pub price: String,
    pub size: String,
    pub order_type: String,
    pub market_title: Option<String>,
    pub outcome: Option<String>,
}

pub async fn init(database_url: &str) -> Result<Db> {
    let options = SqliteConnectOptions::from_str(database_url)?.create_if_missing(true);
    let pool = SqlitePool::connect_with(options).await?;

    sqlx::query("PRAGMA foreign_keys = ON;")
        .execute(&pool)
        .await?;

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

pub async fn set_managed_wallet(
    db: &Db,
    user_id: i64,
    wallet_address: &str,
    encrypted_key: &[u8],
    nonce: &[u8],
    label: Option<&str>,
    signature_type: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO managed_wallets (user_id, wallet_address, encrypted_key, nonce, label, signature_type) VALUES (?, ?, ?, ?, ?, ?)\
         ON CONFLICT(user_id) DO UPDATE SET wallet_address = excluded.wallet_address, encrypted_key = excluded.encrypted_key, nonce = excluded.nonce, label = excluded.label, signature_type = excluded.signature_type",
    )
    .bind(user_id)
    .bind(wallet_address)
    .bind(encrypted_key)
    .bind(nonce)
    .bind(label)
    .bind(signature_type)
    .execute(db)
    .await?;

    Ok(())
}

pub async fn update_managed_wallet_label(db: &Db, user_id: i64, label: Option<&str>) -> Result<()> {
    sqlx::query("UPDATE managed_wallets SET label = ? WHERE user_id = ?")
        .bind(label)
        .bind(user_id)
        .execute(db)
        .await?;

    Ok(())
}

pub async fn get_managed_wallet(db: &Db, user_id: i64) -> Result<Option<models::ManagedWallet>> {
    let wallet = sqlx::query_as::<_, models::ManagedWallet>(
        "SELECT id, user_id, wallet_address, label, signature_type, encrypted_key, nonce, created_at FROM managed_wallets WHERE user_id = ? ORDER BY created_at DESC LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(db)
    .await?;

    Ok(wallet)
}

pub async fn list_managed_wallets_with_users(db: &Db) -> Result<Vec<ManagedWalletWithUser>> {
    let wallets = sqlx::query_as::<_, ManagedWalletWithUser>(
        "SELECT managed_wallets.user_id, users.chat_id, users.telegram_id, managed_wallets.wallet_address, \
         managed_wallets.label, managed_wallets.encrypted_key, managed_wallets.nonce \
         FROM managed_wallets \
         INNER JOIN users ON users.id = managed_wallets.user_id",
    )
    .fetch_all(db)
    .await?;

    Ok(wallets)
}

pub async fn remove_managed_wallet(db: &Db, user_id: i64) -> Result<bool> {
    let result = sqlx::query("DELETE FROM managed_wallets WHERE user_id = ?")
        .bind(user_id)
        .execute(db)
        .await?;

    Ok(result.rows_affected() > 0)
}

pub async fn update_managed_wallet_signature_type(
    db: &Db,
    user_id: i64,
    signature_type: i64,
) -> Result<bool> {
    let result = sqlx::query("UPDATE managed_wallets SET signature_type = ? WHERE user_id = ?")
        .bind(signature_type)
        .bind(user_id)
        .execute(db)
        .await?;

    Ok(result.rows_affected() > 0)
}

pub async fn list_tracked_wallets_with_users(db: &Db) -> Result<Vec<TrackedWalletWithUser>> {
    let wallets = sqlx::query_as::<_, TrackedWalletWithUser>(
        "SELECT tracked_wallets.user_id, users.chat_id, users.telegram_id, tracked_wallets.wallet_address, \
         tracked_wallets.label, tracked_wallets.last_activity_hash, tracked_wallets.last_positions_hash \
         FROM tracked_wallets \
         INNER JOIN users ON users.id = tracked_wallets.user_id",
    )
    .fetch_all(db)
    .await?;

    Ok(wallets)
}

pub async fn remove_tracked_wallet(db: &Db, user_id: i64, wallet_address: &str) -> Result<bool> {
    let result =
        sqlx::query("DELETE FROM tracked_wallets WHERE user_id = ? AND wallet_address = ?")
            .bind(user_id)
            .bind(wallet_address)
            .execute(db)
            .await?;

    Ok(result.rows_affected() > 0)
}

async fn update_tracked_wallet_hash(
    db: &Db,
    user_id: i64,
    wallet_address: &str,
    hash: Option<&str>,
    column: &str,
) -> Result<()> {
    let query =
        format!("UPDATE tracked_wallets SET {column} = ? WHERE user_id = ? AND wallet_address = ?");
    sqlx::query(&query)
        .bind(hash)
        .bind(user_id)
        .bind(wallet_address)
        .execute(db)
        .await?;

    Ok(())
}

pub async fn update_tracked_wallet_activity_hash(
    db: &Db,
    user_id: i64,
    wallet_address: &str,
    hash: Option<&str>,
) -> Result<()> {
    update_tracked_wallet_hash(db, user_id, wallet_address, hash, "last_activity_hash").await
}

pub async fn update_tracked_wallet_positions_hash(
    db: &Db,
    user_id: i64,
    wallet_address: &str,
    hash: Option<&str>,
) -> Result<()> {
    update_tracked_wallet_hash(db, user_id, wallet_address, hash, "last_positions_hash").await
}

pub async fn insert_callback_data(
    db: &Db,
    user_id: i64,
    wallet_address: &str,
    condition_id: &str,
    token_id: Option<&str>,
    side: Option<&str>,
    price: Option<&str>,
    size: Option<&str>,
    market_title: Option<&str>,
    outcome: Option<&str>,
) -> Result<i64> {
    let result = sqlx::query(
        "INSERT INTO callback_data (user_id, wallet_address, condition_id, token_id, side, price, size, market_title, outcome) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(user_id)
    .bind(wallet_address)
    .bind(condition_id)
    .bind(token_id)
    .bind(side)
    .bind(price)
    .bind(size)
    .bind(market_title)
    .bind(outcome)
    .execute(db)
    .await?;

    Ok(result.last_insert_rowid())
}

pub async fn get_callback_data(db: &Db, id: i64) -> Result<Option<CallbackData>> {
    let row = sqlx::query_as::<_, CallbackData>(
        "SELECT user_id, wallet_address, condition_id, token_id, side, price, size, market_title, outcome FROM callback_data WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(db)
    .await?;

    Ok(row)
}

pub async fn insert_copy_trade_state(
    db: &Db,
    user_id: i64,
    token_id: &str,
    side: &str,
    price: &str,
    size: &str,
    order_type: &str,
    market_title: Option<&str>,
    outcome: Option<&str>,
) -> Result<i64> {
    let result = sqlx::query(
        "INSERT INTO copy_trade_state (user_id, token_id, side, price, size, order_type, market_title, outcome) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(user_id)
    .bind(token_id)
    .bind(side)
    .bind(price)
    .bind(size)
    .bind(order_type)
    .bind(market_title)
    .bind(outcome)
    .execute(db)
    .await?;

    Ok(result.last_insert_rowid())
}

pub async fn get_copy_trade_state(db: &Db, id: i64) -> Result<Option<CopyTradeState>> {
    let row = sqlx::query_as::<_, CopyTradeState>(
        "SELECT id, user_id, token_id, side, price, size, order_type, market_title, outcome \
         FROM copy_trade_state WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(db)
    .await?;

    Ok(row)
}

pub enum CopyTradeField {
    Side,
    Price,
    Size,
    OrderType,
}

pub async fn update_copy_trade_field(
    db: &Db,
    id: i64,
    field: CopyTradeField,
    value: &str,
) -> Result<bool> {
    let query = match field {
        CopyTradeField::Side => "UPDATE copy_trade_state SET side = ? WHERE id = ?",
        CopyTradeField::Price => "UPDATE copy_trade_state SET price = ? WHERE id = ?",
        CopyTradeField::Size => "UPDATE copy_trade_state SET size = ? WHERE id = ?",
        CopyTradeField::OrderType => "UPDATE copy_trade_state SET order_type = ? WHERE id = ?",
    };
    let result = sqlx::query(query).bind(value).bind(id).execute(db).await?;

    Ok(result.rows_affected() > 0)
}

pub async fn delete_copy_trade_state(db: &Db, id: i64) -> Result<bool> {
    let result = sqlx::query("DELETE FROM copy_trade_state WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;

    Ok(result.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn setup_db() -> (Db, i64) {
        let options =
            SqliteConnectOptions::from_str("sqlite::memory:").expect("valid sqlite memory options");
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("connect sqlite memory");

        sqlx::query("PRAGMA foreign_keys = ON;")
            .execute(&pool)
            .await
            .expect("enable foreign keys");
        sqlx::migrate!("./src/db/migrations")
            .run(&pool)
            .await
            .expect("run migrations");

        let user_id = ensure_user(&pool, 123, 456).await.expect("insert user");
        (pool, user_id)
    }

    #[tokio::test]
    async fn managed_wallet_set_and_get_persists_signature_type() {
        let (db, user_id) = setup_db().await;

        set_managed_wallet(&db, user_id, "0xabc", b"key", b"nonce", None, 1)
            .await
            .expect("set managed wallet");

        let wallet = get_managed_wallet(&db, user_id)
            .await
            .expect("get managed wallet")
            .expect("wallet exists");

        assert_eq!(wallet.wallet_address, "0xabc");
        assert_eq!(wallet.signature_type, 1);
    }

    #[tokio::test]
    async fn managed_wallet_set_replaces_existing_wallet() {
        let (db, user_id) = setup_db().await;

        set_managed_wallet(&db, user_id, "0xfirst", b"key", b"nonce", None, 0)
            .await
            .expect("set managed wallet");
        set_managed_wallet(&db, user_id, "0xsecond", b"key2", b"nonce2", None, 0)
            .await
            .expect("set managed wallet again");

        let wallet = get_managed_wallet(&db, user_id)
            .await
            .expect("get managed wallet")
            .expect("wallet exists");

        assert_eq!(wallet.wallet_address, "0xsecond");
    }

    #[tokio::test]
    async fn managed_wallet_signature_type_updates() {
        let (db, user_id) = setup_db().await;

        set_managed_wallet(&db, user_id, "0xabc", b"key", b"nonce", None, 0)
            .await
            .expect("set managed wallet");

        let updated = update_managed_wallet_signature_type(&db, user_id, 1)
            .await
            .expect("update signature type");

        assert!(updated);
        let wallet = get_managed_wallet(&db, user_id)
            .await
            .expect("get managed wallet")
            .expect("wallet exists");
        assert_eq!(wallet.signature_type, 1);
    }

    #[tokio::test]
    async fn managed_wallet_remove_clears_wallet() {
        let (db, user_id) = setup_db().await;

        set_managed_wallet(&db, user_id, "0xabc", b"key", b"nonce", None, 0)
            .await
            .expect("set managed wallet");

        let removed = remove_managed_wallet(&db, user_id)
            .await
            .expect("remove managed wallet");

        assert!(removed);
        let wallet = get_managed_wallet(&db, user_id)
            .await
            .expect("get managed wallet");
        assert!(wallet.is_none());
    }

    #[tokio::test]
    async fn callback_data_insert_and_get() {
        let (db, user_id) = setup_db().await;

        let id = insert_callback_data(
            &db, user_id, "0xabc", "0xcond", None, None, None, None, None, None,
        )
        .await
        .expect("insert callback_data");

        let data = get_callback_data(&db, id)
            .await
            .expect("get callback_data")
            .expect("row exists");

        assert_eq!(data.wallet_address, "0xabc");
        assert_eq!(data.condition_id, "0xcond");
        assert_eq!(data.user_id, user_id);
        assert!(data.token_id.is_none());
        assert!(data.side.is_none());
    }

    #[tokio::test]
    async fn callback_data_with_trade_fields() {
        let (db, user_id) = setup_db().await;

        let id = insert_callback_data(
            &db,
            user_id,
            "0xabc",
            "0xcond",
            Some("12345"),
            Some("Buy"),
            Some("0.47"),
            Some("100"),
            Some("Team A vs B"),
            Some("Team A"),
        )
        .await
        .expect("insert callback_data");

        let data = get_callback_data(&db, id)
            .await
            .expect("get callback_data")
            .expect("row exists");

        assert_eq!(data.token_id.as_deref(), Some("12345"));
        assert_eq!(data.side.as_deref(), Some("Buy"));
        assert_eq!(data.price.as_deref(), Some("0.47"));
        assert_eq!(data.size.as_deref(), Some("100"));
        assert_eq!(data.market_title.as_deref(), Some("Team A vs B"));
        assert_eq!(data.outcome.as_deref(), Some("Team A"));
    }

    #[tokio::test]
    async fn callback_data_not_found() {
        let (db, _user_id) = setup_db().await;

        let data = get_callback_data(&db, 99999)
            .await
            .expect("get callback_data");

        assert!(data.is_none());
    }

    #[tokio::test]
    async fn copy_trade_state_insert_and_get() {
        let (db, user_id) = setup_db().await;

        let id = insert_copy_trade_state(
            &db,
            user_id,
            "token123",
            "Buy",
            "0.47",
            "100",
            "limit",
            Some("Team A vs B"),
            Some("Team A"),
        )
        .await
        .expect("insert copy_trade_state");

        let state = get_copy_trade_state(&db, id)
            .await
            .expect("get copy_trade_state")
            .expect("row exists");

        assert_eq!(state.user_id, user_id);
        assert_eq!(state.token_id, "token123");
        assert_eq!(state.side, "Buy");
        assert_eq!(state.price, "0.47");
        assert_eq!(state.size, "100");
        assert_eq!(state.order_type, "limit");
        assert_eq!(state.market_title.as_deref(), Some("Team A vs B"));
        assert_eq!(state.outcome.as_deref(), Some("Team A"));
    }

    #[tokio::test]
    async fn copy_trade_state_not_found() {
        let (db, _user_id) = setup_db().await;

        let state = get_copy_trade_state(&db, 99999)
            .await
            .expect("get copy_trade_state");

        assert!(state.is_none());
    }

    #[tokio::test]
    async fn copy_trade_state_update_side() {
        let (db, user_id) = setup_db().await;

        let id = insert_copy_trade_state(
            &db, user_id, "token123", "Buy", "0.47", "100", "limit", None, None,
        )
        .await
        .expect("insert");

        let updated = update_copy_trade_field(&db, id, CopyTradeField::Side, "Sell")
            .await
            .expect("update side");
        assert!(updated);

        let state = get_copy_trade_state(&db, id)
            .await
            .expect("get")
            .expect("exists");
        assert_eq!(state.side, "Sell");
    }

    #[tokio::test]
    async fn copy_trade_state_update_price() {
        let (db, user_id) = setup_db().await;

        let id = insert_copy_trade_state(
            &db, user_id, "token123", "Buy", "0.47", "100", "limit", None, None,
        )
        .await
        .expect("insert");

        let updated = update_copy_trade_field(&db, id, CopyTradeField::Price, "0.55")
            .await
            .expect("update price");
        assert!(updated);

        let state = get_copy_trade_state(&db, id)
            .await
            .expect("get")
            .expect("exists");
        assert_eq!(state.price, "0.55");
    }

    #[tokio::test]
    async fn copy_trade_state_update_size() {
        let (db, user_id) = setup_db().await;

        let id = insert_copy_trade_state(
            &db, user_id, "token123", "Buy", "0.47", "100", "limit", None, None,
        )
        .await
        .expect("insert");

        let updated = update_copy_trade_field(&db, id, CopyTradeField::Size, "200")
            .await
            .expect("update size");
        assert!(updated);

        let state = get_copy_trade_state(&db, id)
            .await
            .expect("get")
            .expect("exists");
        assert_eq!(state.size, "200");
    }

    #[tokio::test]
    async fn copy_trade_state_update_order_type() {
        let (db, user_id) = setup_db().await;

        let id = insert_copy_trade_state(
            &db, user_id, "token123", "Buy", "0.47", "100", "limit", None, None,
        )
        .await
        .expect("insert");

        let updated = update_copy_trade_field(&db, id, CopyTradeField::OrderType, "market")
            .await
            .expect("update order_type");
        assert!(updated);

        let state = get_copy_trade_state(&db, id)
            .await
            .expect("get")
            .expect("exists");
        assert_eq!(state.order_type, "market");
    }

    #[tokio::test]
    async fn copy_trade_state_update_nonexistent() {
        let (db, _user_id) = setup_db().await;

        let updated = update_copy_trade_field(&db, 99999, CopyTradeField::Side, "Sell")
            .await
            .expect("update nonexistent");
        assert!(!updated);
    }

    #[tokio::test]
    async fn copy_trade_state_delete() {
        let (db, user_id) = setup_db().await;

        let id = insert_copy_trade_state(
            &db, user_id, "token123", "Buy", "0.47", "100", "limit", None, None,
        )
        .await
        .expect("insert");

        let deleted = delete_copy_trade_state(&db, id).await.expect("delete");
        assert!(deleted);

        let state = get_copy_trade_state(&db, id).await.expect("get");
        assert!(state.is_none());
    }

    #[tokio::test]
    async fn copy_trade_state_delete_nonexistent() {
        let (db, _user_id) = setup_db().await;

        let deleted = delete_copy_trade_state(&db, 99999).await.expect("delete");
        assert!(!deleted);
    }
}

pub async fn activity_log_exists(
    db: &Db,
    user_id: i64,
    wallet_address: &str,
    transaction_hash: &str,
    activity_timestamp: i64,
) -> Result<bool> {
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM activity_log \
         WHERE user_id = ? AND wallet_address = ? AND transaction_hash = ? AND activity_timestamp = ?",
    )
    .bind(user_id)
    .bind(wallet_address)
    .bind(transaction_hash)
    .bind(activity_timestamp)
    .fetch_one(db)
    .await?;

    Ok(count > 0)
}

pub async fn insert_activity_log(
    db: &Db,
    user_id: i64,
    wallet_address: &str,
    activity_type: &str,
    market_slug: Option<&str>,
    transaction_hash: &str,
    activity_timestamp: i64,
    details: Option<&str>,
    notified: bool,
) -> Result<bool> {
    let result = sqlx::query(
        "INSERT OR IGNORE INTO activity_log (user_id, wallet_address, activity_type, market_slug, \
         transaction_hash, activity_timestamp, details, notified) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(user_id)
    .bind(wallet_address)
    .bind(activity_type)
    .bind(market_slug)
    .bind(transaction_hash)
    .bind(activity_timestamp)
    .bind(details)
    .bind(notified)
    .execute(db)
    .await?;

    Ok(result.rows_affected() > 0)
}
