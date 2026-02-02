pub mod models;

use color_eyre::eyre::Result;
use sqlx::SqlitePool;

pub type Db = SqlitePool;

pub async fn init(database_url: &str) -> Result<Db> {
    let pool = SqlitePool::connect(database_url).await?;
    
    // Run migrations
    sqlx::migrate!("./src/db/migrations").run(&pool).await?;
    
    Ok(pool)
}
