use polymarket_client_sdk::data::types::request::ActivityRequest;
use polymarket_client_sdk::data::Client as DataClient;
use polymarket_client_sdk::types::Address;
use sqlx::{sqlite::SqlitePoolOptions, Row};
use std::env;
use std::str::FromStr;

fn normalize_database_url(raw: String) -> String {
    if raw.starts_with("sqlite::") || raw.starts_with("sqlite://") {
        return raw;
    }

    if let Some(stripped) = raw.strip_prefix("sqlite:") {
        return format!("sqlite://{}", stripped);
    }

    if raw.contains("://") {
        return raw;
    }

    format!("sqlite://{}", raw)
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    dotenv::dotenv().ok();

    let database_url =
        env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://bot.db".to_string());
    let database_url = normalize_database_url(database_url);
    let poll_seconds = env::var("POLYMARKET_DATA_POLL_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(1);

    let wallet_filter = env::args().nth(1).map(|value| value.to_lowercase());

    println!("DB: {}", database_url);
    println!("POLYMARKET_DATA_POLL_SECONDS: {}", poll_seconds);
    if let Some(filter) = wallet_filter.as_deref() {
        println!("Wallet filter: {}", filter);
    }

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await?;

    let rows = sqlx::query(
        "SELECT tracked_wallets.user_id, users.chat_id, tracked_wallets.wallet_address,\
         tracked_wallets.label, tracked_wallets.last_activity_hash \
         FROM tracked_wallets INNER JOIN users ON users.id = tracked_wallets.user_id",
    )
    .fetch_all(&pool)
    .await?;

    if rows.is_empty() {
        println!("No tracked wallets found.");
        return Ok(());
    }

    let client = DataClient::default();

    for row in rows {
        let wallet_address: String = row.try_get("wallet_address")?;
        let last_activity_hash: Option<String> = row.try_get("last_activity_hash")?;
        let label: Option<String> = row.try_get("label")?;

        let wallet_address = wallet_address.to_lowercase();
        if let Some(filter) = wallet_filter.as_deref() {
            if wallet_address != filter {
                continue;
            }
        }

        println!("\nWallet: {}", wallet_address);
        if let Some(label) = label.as_deref() {
            println!("Label: {}", label);
        }
        println!(
            "Last activity hash in DB: {}",
            last_activity_hash.as_deref().unwrap_or("None")
        );

        let address = match Address::from_str(&wallet_address) {
            Ok(address) => address,
            Err(_) => {
                println!("Invalid wallet address.");
                continue;
            }
        };

        let request = ActivityRequest::builder().user(address).limit(20)?.build();
        let activities = match client.activity(&request).await {
            Ok(activities) => activities,
            Err(err) => {
                println!("Data API error: {err}");
                continue;
            }
        };

        if activities.is_empty() {
            println!("No activities returned by Data API.");
            continue;
        }

        let latest_hash = activities
            .first()
            .map(|activity| activity.transaction_hash.to_string());
        println!(
            "Latest activity hash from API: {}",
            latest_hash.as_deref().unwrap_or("None")
        );

        let mut new_count = 0usize;
        for activity in &activities {
            let hash = activity.transaction_hash.to_string();
            if last_activity_hash.as_deref() == Some(hash.as_str()) {
                break;
            }
            new_count += 1;
        }

        println!("New activities since last hash: {}", new_count);
        println!("Top 5 activities:");
        for activity in activities.iter().take(5) {
            let market = activity
                .title
                .clone()
                .or_else(|| activity.slug.clone())
                .unwrap_or_else(|| "Unknown market".to_string());
            println!(
                "- {:?} | {} | {} | {}",
                activity.activity_type,
                activity.timestamp,
                activity.transaction_hash,
                market
            );
        }
    }

    Ok(())
}
