use futures::StreamExt;
use polymarket_client_sdk::clob::ws::Client;
use polymarket_client_sdk::types::U256;
use std::str::FromStr;

use crate::db::Db;

pub fn spawn_monitoring(_db: Db, asset_ids: Vec<String>) -> Option<tokio::task::JoinHandle<()>> {
    if asset_ids.is_empty() {
        return None;
    }

    Some(tokio::spawn(async move {
        let parsed: Vec<U256> = asset_ids
            .into_iter()
            .filter_map(|asset_id| U256::from_str(&asset_id).ok())
            .collect();

        if parsed.is_empty() {
            return;
        }

        let client = Client::default();
        let stream = match client.subscribe_orderbook(parsed) {
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
