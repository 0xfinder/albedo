use futures::StreamExt;
use polymarket_client_sdk::auth::{Credentials, Uuid};
use polymarket_client_sdk::clob::ws::Client as WsClient;
use polymarket_client_sdk::data::types::request::{ActivityRequest, PositionsRequest};
use polymarket_client_sdk::data::Client as DataClient;
use polymarket_client_sdk::types::{Address, Decimal};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::str::FromStr;
use std::time::Duration;
use teloxide::prelude::Requester;

use crate::config::WsCredentialsConfig;
use crate::db::{self, Db};

pub fn spawn_data_polling(
    bot: teloxide::prelude::Bot,
    db: Db,
    poll_seconds: u64,
) -> Option<tokio::task::JoinHandle<()>> {
    if poll_seconds == 0 {
        return None;
    }

    Some(tokio::spawn(async move {
        let client = DataClient::default();
        let mut interval = tokio::time::interval(Duration::from_secs(poll_seconds));

        loop {
            interval.tick().await;

            let wallets = match db::list_tracked_wallets_with_users(&db).await {
                Ok(wallets) => wallets,
                Err(_) => continue,
            };

            for wallet in wallets {
                let address = match Address::from_str(&wallet.wallet_address) {
                    Ok(address) => address,
                    Err(_) => continue,
                };

                let _ = poll_activity(&bot, &client, &db, &wallet, address).await;
                let _ = poll_positions(&bot, &client, &db, &wallet, address).await;

                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    }))
}

pub fn spawn_ws_user_events(
    _bot: teloxide::prelude::Bot,
    _db: Db,
    credentials: Option<WsCredentialsConfig>,
) -> Option<tokio::task::JoinHandle<()>> {
    let credentials = credentials?;

    Some(tokio::spawn(async move {
        let api_key = match Uuid::parse_str(&credentials.api_key) {
            Ok(api_key) => api_key,
            Err(_) => return,
        };

        let address = match Address::from_str(&credentials.address) {
            Ok(address) => address,
            Err(_) => return,
        };

        let credentials = Credentials::new(
            api_key,
            credentials.api_secret,
            credentials.api_passphrase,
        );

        let client = match WsClient::default().authenticate(credentials, address) {
            Ok(client) => client,
            Err(_) => return,
        };

        let stream = match client.subscribe_user_events(Vec::new()) {
            Ok(stream) => stream,
            Err(_) => return,
        };

        let mut stream = Box::pin(stream);
        while let Some(event) = stream.next().await {
            if event.is_err() {
                break;
            }
        }
    }))
}

async fn poll_activity(
    bot: &teloxide::prelude::Bot,
    client: &DataClient,
    db: &Db,
    wallet: &db::TrackedWalletWithUser,
    address: Address,
) -> color_eyre::eyre::Result<()> {
    let request = ActivityRequest::builder().user(address).limit(20)?.build();
    let activities = match client.activity(&request).await {
        Ok(activities) => activities,
        Err(_) => return Ok(()),
    };

    if activities.is_empty() {
        return Ok(());
    }

    let latest_hash = activities
        .first()
        .map(|activity| activity.transaction_hash.to_string());

    let last_hash = wallet.last_activity_hash.as_deref();
    if last_hash.is_none() {
        if let Some(hash) = latest_hash.as_deref() {
            let _ = db::update_tracked_wallet_activity_hash(
                db,
                wallet.user_id,
                &wallet.wallet_address,
                Some(hash),
            )
            .await;
        }
        return Ok(());
    }

    let mut new_events = Vec::new();
    for activity in &activities {
        let hash = activity.transaction_hash.to_string();
        if Some(hash.as_str()) == last_hash {
            break;
        }
        new_events.push(activity);
    }

    if let Some(hash) = latest_hash.as_deref() {
        let _ = db::update_tracked_wallet_activity_hash(
            db,
            wallet.user_id,
            &wallet.wallet_address,
            Some(hash),
        )
        .await;
    }

    if new_events.is_empty() {
        return Ok(());
    }

    let label = wallet
        .label
        .as_deref()
        .unwrap_or(wallet.wallet_address.as_str());
    for activity in new_events.into_iter().rev() {
        let title = activity.title.as_deref().unwrap_or("Unknown market");
        let side = activity
            .side
            .as_ref()
            .map(|side| format!("{:?}", side))
            .unwrap_or_else(|| "N/A".to_string());
        let size = format_decimal(activity.size);
        let price = activity
            .price
            .map(format_decimal)
            .unwrap_or_else(|| "N/A".to_string());
        let message = format!(
            "New activity for {label}: {activity:?}\n{title}\nSide: {side} Size: {size} Price: {price}",
            activity = activity.activity_type
        );
        let _ = bot
            .send_message(teloxide::types::ChatId(wallet.chat_id), message)
            .await;
    }

    Ok(())
}

async fn poll_positions(
    bot: &teloxide::prelude::Bot,
    client: &DataClient,
    db: &Db,
    wallet: &db::TrackedWalletWithUser,
    address: Address,
) -> color_eyre::eyre::Result<()> {
    let request = PositionsRequest::builder().user(address).limit(200)?.build();
    let positions = match client.positions(&request).await {
        Ok(positions) => positions,
        Err(_) => return Ok(()),
    };

    let hash = hash_positions(&positions);
    let hash_string = hash.to_string();

    let last_hash = wallet.last_positions_hash.as_deref();
    if last_hash == Some(hash_string.as_str()) {
        return Ok(());
    }

    let _ = db::update_tracked_wallet_positions_hash(
        db,
        wallet.user_id,
        &wallet.wallet_address,
        Some(&hash_string),
    )
    .await;

    if last_hash.is_none() {
        return Ok(());
    }

    let label = wallet
        .label
        .as_deref()
        .unwrap_or(wallet.wallet_address.as_str());
    let message = format!(
        "Positions updated for {label}. Open positions: {count}.",
        count = positions.len()
    );
    let _ = bot
        .send_message(teloxide::types::ChatId(wallet.chat_id), message)
        .await;

    Ok(())
}

fn hash_positions(positions: &[polymarket_client_sdk::data::types::response::Position]) -> u64 {
    let mut entries: Vec<String> = positions
        .iter()
        .map(|position| {
            format!(
                "{}:{}:{}:{}",
                position.condition_id,
                position.outcome_index,
                position.size,
                position.avg_price
            )
        })
        .collect();
    entries.sort();

    let mut hasher = DefaultHasher::new();
    for entry in entries {
        entry.hash(&mut hasher);
    }
    hasher.finish()
}

fn format_decimal(value: Decimal) -> String {
    value.normalize().to_string()
}
