//! Data API v2 transport, adapting wire names to the SDK's domain types.

use color_eyre::eyre::{Result, ensure};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Map, Value};
use std::time::Duration;

#[derive(Debug, Clone)]
pub(crate) struct DataClient {
    http: reqwest::Client,
    pub(crate) health: crate::health::SharedHealth,
}

impl Default for DataClient {
    fn default() -> Self {
        Self {
            health: std::sync::Arc::default(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("HTTP client"),
        }
    }
}

#[derive(Deserialize)]
struct Page {
    data: Vec<Map<String, Value>>,
    pagination: Pagination,
}

#[derive(Deserialize)]
struct Pagination {
    next_cursor: Option<String>,
}

impl DataClient {
    pub(crate) async fn fetch<T: DeserializeOwned>(
        &self,
        endpoint: &str,
        request: &impl Serialize,
        all_pages: bool,
    ) -> Result<Vec<T>> {
        let query = query_params(request)?;
        let key = query
            .get("user")
            .filter(|_| matches!(endpoint, "activity" | "positions"))
            .map(|user| crate::health::Key::Fetch(user.to_ascii_lowercase(), endpoint.into()));
        let result = self.fetch_pages(endpoint, query, all_pages).await;
        if let Some(key) = key {
            self.health.lock().await.record(
                key,
                result.as_ref().err().map(ToString::to_string),
                std::time::Instant::now(),
            );
        }
        result
    }

    async fn fetch_pages<T: DeserializeOwned>(
        &self,
        endpoint: &str,
        mut query: std::collections::BTreeMap<String, String>,
        all_pages: bool,
    ) -> Result<Vec<T>> {
        let mut result = Vec::new();
        loop {
            let page: Page = self
                .http
                .get(format!("https://data-api.polymarket.com/v2/{endpoint}"))
                .query(&query)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            for row in page.data {
                result.push(serde_json::from_value(adapt_row(row))?);
            }
            if !all_pages {
                break;
            }
            let Some(cursor) = page.pagination.next_cursor else {
                break;
            };
            query.insert("cursor".into(), cursor);
        }
        Ok(result)
    }
}

fn query_params(request: &impl Serialize) -> Result<std::collections::BTreeMap<String, String>> {
    let Value::Object(params) = serde_json::to_value(request)? else {
        color_eyre::eyre::bail!("expected request object");
    };
    let mut query = std::collections::BTreeMap::new();
    for (key, value) in params {
        if key == "offset" {
            ensure!(value == 0, "v2 requires cursor pagination");
            continue;
        }
        let key = match key.as_str() {
            "market" => "condition",
            "sizeThreshold" => "filter_amount",
            "redeemable" | "mergeable" => {
                if value == true {
                    query.insert("status".into(), key.to_uppercase());
                }
                continue;
            }
            _ => &key,
        };
        let value = match value {
            Value::String(s) => s,
            other => other.to_string(),
        };
        query.insert(key.into(), value);
    }
    Ok(query)
}

fn adapt_row(row: Map<String, Value>) -> Value {
    let mut mapped = Map::new();
    for (key, value) in row {
        let name = match key.as_str() {
            "token_id" => "asset".into(),
            "opposite_token_id" => "oppositeAsset".into(),
            "current_size" => "size".into(),
            "entry_cost_usdc" => "initialValue".into(),
            "unrealized_pnl" => "cashPnl".into(),
            "total_size" => "totalBought".into(),
            "current_price" => "curPrice".into(),
            _ => {
                let mut parts = key.split('_');
                let mut name = parts.next().unwrap_or_default().to_owned();
                for part in parts {
                    let mut chars = part.chars();
                    if let Some(first) = chars.next() {
                        name.extend(first.to_uppercase());
                        name.extend(chars);
                    }
                }
                name
            }
        };
        mapped.insert(name, value);
    }
    Value::Object(mapped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use polymarket_client_sdk::data::types::request::PositionsRequest;
    use polymarket_client_sdk::data::types::response::{Activity, Position, Trade};
    use polymarket_client_sdk::types::Decimal;

    #[test]
    fn decodes_v2_responses() {
        let activity: Activity = serde_json::from_value(adapt_row(
            serde_json::from_str(include_str!("fixtures/activity.json")).unwrap(),
        ))
        .unwrap();
        assert!(activity.size >= Decimal::ZERO);
        let position: Position = serde_json::from_value(adapt_row(
            serde_json::from_str(include_str!("fixtures/positions.json")).unwrap(),
        ))
        .unwrap();
        let wire: Value = serde_json::from_str(include_str!("fixtures/positions.json")).unwrap();
        assert_eq!(
            position.size,
            serde_json::from_value::<Decimal>(wire["current_size"].clone()).unwrap()
        );
        let _: Trade = serde_json::from_value(adapt_row(
            serde_json::from_str(include_str!("fixtures/trades.json")).unwrap(),
        ))
        .unwrap();
    }

    #[tokio::test]
    #[ignore = "requires public Polymarket API access"]
    async fn live_v2_reads() {
        use polymarket_client_sdk::data::types::request::{ActivityRequest, TradesRequest};
        let client = DataClient::default();
        let trades: Vec<Trade> = client
            .fetch(
                "trades",
                &TradesRequest::builder().limit(1).unwrap().build(),
                false,
            )
            .await
            .unwrap();
        let user = trades.first().expect("global trade feed").proxy_wallet;
        let _: Vec<Activity> = client
            .fetch(
                "activity",
                &ActivityRequest::builder()
                    .user(user)
                    .limit(1)
                    .unwrap()
                    .build(),
                false,
            )
            .await
            .unwrap();
        let _: Vec<Position> = client
            .fetch(
                "positions",
                &PositionsRequest::builder()
                    .user(user)
                    .limit(500)
                    .unwrap()
                    .build(),
                true,
            )
            .await
            .unwrap();
    }

    #[test]
    fn translates_position_filter() {
        let request = PositionsRequest::builder()
            .user(polymarket_client_sdk::types::Address::ZERO)
            .size_threshold(Decimal::ONE)
            .redeemable(true)
            .offset(0)
            .unwrap()
            .build();
        let params = query_params(&request).unwrap();
        assert_eq!(params["filter_amount"], "1");
        assert_eq!(params["status"], "REDEEMABLE");
        assert!(!params.contains_key("offset"));
    }
}
