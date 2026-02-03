use futures::{Stream, StreamExt};
use polymarket_client_sdk::auth::{Credentials, LocalSigner, Signer, Uuid};
use polymarket_client_sdk::clob::Client as ClobClient;
use polymarket_client_sdk::clob::ws::Client as WsClient;
use polymarket_client_sdk::data::types::request::{ActivityRequest, PositionsRequest};
use polymarket_client_sdk::data::Client as DataClient;
use polymarket_client_sdk::types::{Address, Decimal};
use polymarket_client_sdk::POLYGON;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::str::FromStr;
use std::time::Duration;
use teloxide::prelude::Requester;

use crate::config::WsCredentialsConfig;
use crate::db::{self, Db};
use crate::utils::crypto::{self, EncryptionKey};

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
    db: Db,
    encryption_key: Option<EncryptionKey>,
    ws_credentials: Option<WsCredentialsConfig>,
) -> Option<tokio::task::JoinHandle<()>> {
    if encryption_key.is_none() && ws_credentials.is_none() {
        return None;
    }

    Some(tokio::spawn(async move {
        if let Some(credentials) = ws_credentials {
            tokio::spawn(async move {
                let _ = connect_env_user_events(credentials).await;
            });
        }

        if let Some(encryption_key) = encryption_key {
            let wallets = match db::list_managed_wallets_with_users(&db).await {
                Ok(wallets) => wallets,
                Err(_) => return,
            };

            for wallet in wallets {
                let encryption_key = encryption_key;
                tokio::spawn(async move {
                    let _ = connect_user_events(wallet, encryption_key).await;
                });
            }
        }
    }))
}

async fn consume_user_event_stream<S, T, E>(mut stream: S) -> usize
where
    S: Stream<Item = Result<T, E>> + Unpin,
{
    let mut ok_count = 0;
    while let Some(event) = stream.next().await {
        if event.is_err() {
            break;
        }
        ok_count += 1;
    }
    ok_count
}

async fn connect_env_user_events(
    credentials: WsCredentialsConfig,
) -> color_eyre::eyre::Result<()> {
    let api_key = Uuid::parse_str(&credentials.api_key)?;
    let address = Address::from_str(&credentials.address)?;
    let credentials = Credentials::new(
        api_key,
        credentials.api_secret,
        credentials.api_passphrase,
    );

    let client = WsClient::default().authenticate(credentials, address)?;
    let stream = Box::pin(client.subscribe_user_events(Vec::new())?);
    let _ = consume_user_event_stream(stream).await;

    Ok(())
}

async fn connect_user_events(
    wallet: db::ManagedWalletWithUser,
    encryption_key: EncryptionKey,
) -> color_eyre::eyre::Result<()> {
    let decrypted = crypto::decrypt(encryption_key, &wallet.nonce, &wallet.encrypted_key)?;
    let private_key = String::from_utf8(decrypted)?;
    let signer = LocalSigner::from_str(&private_key)?.with_chain_id(Some(POLYGON));

    let address = signer.address();
    let credentials = ClobClient::default()
        .create_or_derive_api_key(&signer, None)
        .await?;

    let client = WsClient::default().authenticate(credentials, address)?;
    let stream = Box::pin(client.subscribe_user_events(Vec::new())?);
    let _ = consume_user_event_stream(stream).await;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::consume_user_event_stream;
    use futures::stream;

    #[tokio::test]
    async fn consume_events_stops_on_error() {
        let events = stream::iter(vec![Ok("one"), Ok("two"), Err("boom"), Ok("three")]);
        let count = consume_user_event_stream(events).await;
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn consume_events_reads_to_end() {
        let events = stream::iter(vec![
            Ok::<u8, &'static str>(1u8),
            Ok(2u8),
            Ok(3u8),
        ]);
        let count = consume_user_event_stream(events).await;
        assert_eq!(count, 3);
    }
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
