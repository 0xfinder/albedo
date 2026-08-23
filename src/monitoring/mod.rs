use futures::{Stream, StreamExt};
use polymarket_client_sdk::auth::{LocalSigner, Signer};
use polymarket_client_sdk::clob::ws::types::response::{OrderMessage, TradeMessage, WsMessage};
use polymarket_client_sdk::clob::ws::Client as WsClient;
use polymarket_client_sdk::clob::Client as ClobClient;
use polymarket_client_sdk::data::types::request::{
    ActivityRequest, PositionsRequest, TradesRequest,
};
use polymarket_client_sdk::data::types::response::Activity;
use polymarket_client_sdk::data::types::MarketFilter;
use polymarket_client_sdk::data::Client as DataClient;
use polymarket_client_sdk::types::{Address, Decimal, B256};
use polymarket_client_sdk::POLYGON;
use serde::Serialize;
use std::collections::{hash_map::DefaultHasher, HashMap};
use std::hash::{Hash, Hasher};
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use teloxide::payloads::SendMessageSetters;
use teloxide::prelude::Requester;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};
use tokio::sync::Mutex;

use crate::db::{self, Db};
use crate::utils::crypto::{self, EncryptionKey};
use crate::utils::number_format;

const WS_BACKOFF_INITIAL_MS: u64 = 1000;
const WS_BACKOFF_MAX_MS: u64 = 30_000;
const WS_BACKOFF_RESET_AFTER_SECS: u64 = 60;

type MarketCache = Arc<Mutex<HashMap<String, MarketInfo>>>;

pub fn spawn_data_polling(
    bot: teloxide::prelude::Bot,
    db: Db,
    poll_interval: Duration,
) -> Option<tokio::task::JoinHandle<()>> {
    if poll_interval.is_zero() {
        return None;
    }

    Some(tokio::spawn(async move {
        let client = DataClient::default();
        let mut interval = tokio::time::interval(poll_interval);

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
    bot: teloxide::prelude::Bot,
    db: Db,
    encryption_key: Option<EncryptionKey>,
) -> Option<tokio::task::JoinHandle<()>> {
    if encryption_key.is_none() {
        return None;
    }

    Some(tokio::spawn(async move {
        if let Some(encryption_key) = encryption_key {
            let wallets = match db::list_managed_wallets_with_users(&db).await {
                Ok(wallets) => wallets,
                Err(_) => return,
            };

            for wallet in wallets {
                let encryption_key = encryption_key.clone();
                let db = db.clone();
                let bot = bot.clone();
                tokio::spawn(async move {
                    let _ = connect_user_events(wallet, db, bot, encryption_key).await;
                });
            }
        }
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamExit {
    End,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StreamOutcome {
    ok_count: usize,
    exit: StreamExit,
}

#[cfg(test)]
async fn consume_user_event_stream<S, T, E>(mut stream: S) -> StreamOutcome
where
    S: Stream<Item = Result<T, E>> + Unpin,
{
    let mut ok_count = 0;
    while let Some(event) = stream.next().await {
        match event {
            Ok(_) => ok_count += 1,
            Err(_) => {
                return StreamOutcome {
                    ok_count,
                    exit: StreamExit::Error,
                }
            }
        }
    }

    StreamOutcome {
        ok_count,
        exit: StreamExit::End,
    }
}

async fn consume_user_event_stream_with_handler<S, F, Fut, E>(
    mut stream: S,
    mut handler: F,
) -> StreamOutcome
where
    S: Stream<Item = Result<WsMessage, E>> + Unpin,
    F: FnMut(WsMessage) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let mut ok_count = 0;
    while let Some(event) = stream.next().await {
        match event {
            Ok(message) => {
                handler(message).await;
                ok_count += 1;
            }
            Err(_) => {
                return StreamOutcome {
                    ok_count,
                    exit: StreamExit::Error,
                }
            }
        }
    }

    StreamOutcome {
        ok_count,
        exit: StreamExit::End,
    }
}

async fn run_ws_with_backoff<F, Fut>(mut connect: F) -> color_eyre::eyre::Result<()>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = color_eyre::eyre::Result<StreamOutcome>>,
{
    let mut backoff = Duration::from_millis(WS_BACKOFF_INITIAL_MS);
    loop {
        let started = Instant::now();
        let _ = connect().await;
        let elapsed = started.elapsed();
        if elapsed >= Duration::from_secs(WS_BACKOFF_RESET_AFTER_SECS) {
            backoff = Duration::from_millis(WS_BACKOFF_INITIAL_MS);
        }

        tokio::time::sleep(backoff).await;
        backoff = std::cmp::min(
            backoff.saturating_mul(2),
            Duration::from_millis(WS_BACKOFF_MAX_MS),
        );
    }
}

async fn connect_user_events(
    wallet: db::ManagedWalletWithUser,
    db: Db,
    bot: teloxide::prelude::Bot,
    encryption_key: EncryptionKey,
) -> color_eyre::eyre::Result<()> {
    run_ws_with_backoff(|| connect_user_events_once(&wallet, &db, &bot, encryption_key.clone()))
        .await
}

async fn connect_user_events_once(
    wallet: &db::ManagedWalletWithUser,
    db: &Db,
    bot: &teloxide::prelude::Bot,
    encryption_key: EncryptionKey,
) -> color_eyre::eyre::Result<StreamOutcome> {
    let aad = crypto::build_aad(wallet.user_id, &wallet.wallet_address);
    let decrypted = crypto::decrypt(&encryption_key, &wallet.nonce, &wallet.encrypted_key, &aad)?;
    let private_key = String::from_utf8(decrypted)?;
    let signer = LocalSigner::from_str(&private_key)?.with_chain_id(Some(POLYGON));

    let address = signer.address();
    let credentials = ClobClient::default()
        .create_or_derive_api_key(&signer, None)
        .await?;

    let client = WsClient::default().authenticate(credentials, address)?;
    let stream = Box::pin(client.subscribe_user_events(Vec::new())?);
    let data_client = DataClient::default();
    let market_cache: MarketCache = Arc::new(Mutex::new(HashMap::new()));

    Ok(consume_user_event_stream_with_handler(stream, |message| {
        let data_client = data_client.clone();
        let market_cache = market_cache.clone();
        async move {
            handle_ws_message(bot, db, wallet, message, &data_client, &market_cache).await;
        }
    })
    .await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;

    #[tokio::test]
    async fn consume_events_stops_on_error() {
        let events = stream::iter(vec![Ok("one"), Ok("two"), Err("boom"), Ok("three")]);
        let outcome = consume_user_event_stream(events).await;
        assert_eq!(outcome.ok_count, 2);
        assert_eq!(outcome.exit, StreamExit::Error);
    }

    #[tokio::test]
    async fn consume_events_reads_to_end() {
        let events = stream::iter(vec![Ok::<u8, &'static str>(1u8), Ok(2u8), Ok(3u8)]);
        let outcome = consume_user_event_stream(events).await;
        assert_eq!(outcome.ok_count, 3);
        assert_eq!(outcome.exit, StreamExit::End);
    }

    #[test]
    fn format_decimal_normalizes() {
        let d = Decimal::from_str("1.50000").unwrap();
        assert_eq!(format_decimal(d), "1.500");
    }

    #[test]
    fn format_decimal_preserves_integer() {
        let d = Decimal::from_str("100").unwrap();
        assert_eq!(format_decimal(d), "100.000");
    }

    #[test]
    fn format_optional_decimal_some() {
        let d = Some(Decimal::from_str("2.5").unwrap());
        assert_eq!(format_optional_decimal(d), "2.500");
    }

    #[test]
    fn format_optional_decimal_none() {
        assert_eq!(format_optional_decimal(None), "N/A");
    }

    #[test]
    fn format_market_label_with_slug() {
        let info = MarketInfo {
            title: "Will Bitcoin hit 100k?".to_string(),
            slug: Some("btc-100k".to_string()),
        };
        assert_eq!(
            format_market_label(&info),
            "Will Bitcoin hit 100k? (btc-100k)"
        );
    }

    #[test]
    fn format_market_label_without_slug() {
        let info = MarketInfo {
            title: "Some Market".to_string(),
            slug: None,
        };
        assert_eq!(format_market_label(&info), "Some Market");
    }

    #[test]
    fn format_activity_message_complete() {
        let notification = ActivityNotification {
            activity_type: "Buy".to_string(),
            market: "Bitcoin 100k".to_string(),
            market_slug: Some("btc-100k".to_string()),
            outcome: Some("Yes".to_string()),
            side: Some("Buy".to_string()),
            size: "10".to_string(),
            usdc_size: "5.50".to_string(),
            price: Some("0.55".to_string()),
            timestamp: 1700000000,
            tx_hash: "0xabc123".to_string(),
            condition_id: None,
            asset: None,
            username: Some("polyuser".to_string()),
        };
        let msg = format_activity_message("0x123abc", Some("my_wallet"), &notification);
        assert!(msg.contains("0x123abc"));
        assert!(msg.contains("my_wallet"));
        assert!(msg.contains("@polyuser"));
        assert!(msg.contains("🟢 <b>BUY</b>"));
        assert!(msg.contains("Bitcoin 100k"));
        assert!(msg.contains("btc-100k"));
        assert!(msg.contains("Yes"));
        assert!(msg.contains("10.000"));
        assert!(msg.contains("$5.500"));
        assert!(msg.contains("$0.550 (1.82)"));
    }

    #[test]
    fn format_activity_message_missing_optional_fields() {
        let notification = ActivityNotification {
            activity_type: "Transfer".to_string(),
            market: "Some Market".to_string(),
            market_slug: None,
            outcome: None,
            side: None,
            size: "100".to_string(),
            usdc_size: "100".to_string(),
            price: None,
            timestamp: 1700000000,
            tx_hash: "0xdef456".to_string(),
            condition_id: None,
            asset: None,
            username: None,
        };
        let msg = format_activity_message("0xwallet", None, &notification);
        assert!(msg.contains("N/A"));
        assert!(!msg.contains("Name:"));
    }

    #[test]
    fn format_activity_message_referral_reward() {
        let notification = ActivityNotification {
            activity_type: "Unknown(\"REFERRAL_REWARD\")".to_string(),
            market: "Some Market".to_string(),
            market_slug: None,
            outcome: None,
            side: None,
            size: "100".to_string(),
            usdc_size: "100".to_string(),
            price: None,
            timestamp: 1700000000,
            tx_hash: "0xdef456".to_string(),
            condition_id: None,
            asset: None,
            username: None,
        };
        let msg = format_activity_message("0xwallet", None, &notification);
        assert!(msg.contains("🎁 <b>REFERRAL REWARD</b>"));
        assert!(msg.contains("Value:"));
        assert!(!msg.contains("Market:"));
        assert!(!msg.contains("Outcome:"));
        assert!(!msg.contains("Size:"));
        assert!(!msg.contains("Price:"));
    }

    #[test]
    fn format_activity_message_maker_rebate() {
        let notification = ActivityNotification {
            activity_type: "Unknown(\"MAKER_REBATE\")".to_string(),
            market: "Some Market".to_string(),
            market_slug: None,
            outcome: None,
            side: None,
            size: "100".to_string(),
            usdc_size: "100".to_string(),
            price: None,
            timestamp: 1700000000,
            tx_hash: "0xdef456".to_string(),
            condition_id: None,
            asset: None,
            username: None,
        };
        let msg = format_activity_message("0xwallet", None, &notification);
        assert!(msg.contains("💰 <b>MAKER REBATE</b>"));
        assert!(msg.contains("Value:"));
        assert!(!msg.contains("Market:"));
        assert!(!msg.contains("Outcome:"));
        assert!(!msg.contains("Size:"));
        assert!(!msg.contains("Price:"));
    }

    #[test]
    fn format_activity_message_taker_rebate() {
        let notification = ActivityNotification {
            activity_type: "Unknown(\"TAKER_REBATE\")".to_string(),
            market: "Some Market".to_string(),
            market_slug: None,
            outcome: None,
            side: None,
            size: "100".to_string(),
            usdc_size: "100".to_string(),
            price: None,
            timestamp: 1700000000,
            tx_hash: "0xdef456".to_string(),
            condition_id: None,
            asset: None,
            username: None,
        };
        let msg = format_activity_message("0xwallet", None, &notification);
        assert!(msg.contains("💵 <b>TAKER REBATE</b>"));
        assert!(msg.contains("Value:"));
        assert!(!msg.contains("Market:"));
        assert!(!msg.contains("Outcome:"));
        assert!(!msg.contains("Size:"));
        assert!(!msg.contains("Price:"));
    }

    #[test]
    fn is_tradeable_activity_buy() {
        assert!(is_tradeable_activity("Buy"));
    }

    #[test]
    fn is_tradeable_activity_sell() {
        assert!(is_tradeable_activity("Sell"));
    }

    #[test]
    fn is_tradeable_activity_trade() {
        assert!(is_tradeable_activity("Trade"));
    }

    #[test]
    fn is_tradeable_activity_redeem() {
        assert!(!is_tradeable_activity("Redeem"));
    }

    #[test]
    fn is_tradeable_activity_claim() {
        assert!(!is_tradeable_activity("Claim"));
    }

    #[test]
    fn is_tradeable_activity_reward() {
        assert!(!is_tradeable_activity("Reward"));
    }
}

async fn poll_activity(
    bot: &teloxide::prelude::Bot,
    client: &DataClient,
    db: &Db,
    wallet: &db::TrackedWalletWithUser,
    address: Address,
) -> color_eyre::eyre::Result<()> {
    let request = ActivityRequest::builder().user(address).limit(500)?.build();
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

    if new_events.is_empty() {
        return Ok(());
    }

    for activity in new_events.into_iter().rev() {
        let notification = ActivityNotification::from_activity(activity);
        let details = serde_json::to_string(&notification).ok();

        match db::activity_log_exists(
            db,
            wallet.user_id,
            &wallet.wallet_address,
            &notification.tx_hash,
            notification.timestamp,
        )
        .await
        {
            Ok(true) => continue,
            Ok(false) => {}
            Err(err) => {
                eprintln!(
                    "activity poll: dedupe check failed for tx {}: {err}",
                    notification.tx_hash
                );
                return Ok(());
            }
        }

        let message = format_activity_message(
            &wallet.wallet_address,
            wallet.label.as_deref(),
            &notification,
        );
        let mut request = bot
            .send_message(teloxide::types::ChatId(wallet.chat_id), message)
            .parse_mode(teloxide::types::ParseMode::Html)
            .link_preview_options(teloxide::types::LinkPreviewOptions {
                is_disabled: true,
                url: None,
                prefer_small_media: false,
                prefer_large_media: false,
                show_above_text: false,
            });
        // Closed markets (Redeem/Claim) have no positions left to show,
        // so skip the button row entirely.
        if !is_closed_activity(&notification.activity_type) {
            if let Some(condition_id) = &notification.condition_id {
                let (trade_token, trade_side, trade_price, trade_size) =
                    if is_tradeable_activity(&notification.activity_type) {
                        (
                            notification.asset.as_deref(),
                            notification.side.as_deref(),
                            notification.price.as_deref(),
                            Some(notification.size.as_str()),
                        )
                    } else {
                        (None, None, None, None)
                    };
                if let Ok(cb_id) = db::insert_callback_data(
                    db,
                    &wallet.wallet_address,
                    condition_id,
                    trade_token,
                    trade_side,
                    trade_price,
                    trade_size,
                    Some(notification.market.as_str()),
                    notification.outcome.as_deref(),
                )
                .await
                {
                    let mut buttons = vec![InlineKeyboardButton::callback(
                        "📊 Show Positions",
                        format!("sp:{cb_id}"),
                    )];
                    if trade_token.is_some() {
                        buttons.push(InlineKeyboardButton::callback(
                            "📋 Copy Trade",
                            format!("ct:{cb_id}"),
                        ));
                    }
                    let markup = InlineKeyboardMarkup::new(vec![buttons]);
                    request = request.reply_markup(markup);
                }
            }
        }

        // Retry transient Telegram failures; on persistent failure stop and
        // leave the cursor at the last delivered event so the next poll resumes
        // here instead of skipping the rest of the batch.
        let mut sent = false;
        for attempt in 0..3 {
            if attempt > 0 {
                tokio::time::sleep(std::time::Duration::from_secs(attempt)).await;
            }
            match request.clone().await {
                Ok(_) => {
                    sent = true;
                    break;
                }
                Err(err) => {
                    eprintln!(
                        "activity poll: send failed (attempt {} of 3) for tx {}: {err}",
                        attempt + 1,
                        notification.tx_hash
                    );
                }
            }
        }
        if !sent {
            return Ok(());
        }

        if let Err(err) = db::insert_activity_log(
            db,
            wallet.user_id,
            &wallet.wallet_address,
            &notification.activity_type,
            notification.market_slug.as_deref(),
            &notification.tx_hash,
            notification.timestamp,
            details.as_deref(),
            true,
        )
        .await
        {
            eprintln!(
                "activity poll: failed to record tx {}: {err}",
                notification.tx_hash
            );
        }

        if let Err(err) = db::update_tracked_wallet_activity_hash(
            db,
            wallet.user_id,
            &wallet.wallet_address,
            Some(&activity.transaction_hash.to_string()),
        )
        .await
        {
            eprintln!(
                "activity poll: failed to advance cursor after tx {}: {err}",
                notification.tx_hash
            );
        }
    }

    Ok(())
}

async fn poll_positions(
    _bot: &teloxide::prelude::Bot,
    client: &DataClient,
    db: &Db,
    wallet: &db::TrackedWalletWithUser,
    address: Address,
) -> color_eyre::eyre::Result<()> {
    let request = PositionsRequest::builder()
        .user(address)
        .limit(200)?
        .build();
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

    Ok(())
}

fn hash_positions(positions: &[polymarket_client_sdk::data::types::response::Position]) -> u64 {
    let mut entries: Vec<String> = positions
        .iter()
        .map(|position| {
            format!(
                "{}:{}:{}:{}",
                position.condition_id, position.outcome_index, position.size, position.avg_price
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
    number_format::format_value(value)
}

fn is_tradeable_activity(activity_type: &str) -> bool {
    matches!(activity_type, "Buy" | "Sell" | "Trade")
}

// Reward/rebate payouts carry no market context, so their notifications
// render a compact wallet + value layout instead of the trade fields.
fn is_reward_activity(activity_type: &str) -> bool {
    matches!(
        activity_type,
        "Unknown(\"REFERRAL_REWARD\")"
            | "REFERRAL_REWARD"
            | "Unknown(\"MAKER_REBATE\")"
            | "MAKER_REBATE"
            | "MakerRebate"
            | "Unknown(\"TAKER_REBATE\")"
            | "TAKER_REBATE"
            | "TakerRebate"
    )
}

fn is_closed_activity(activity_type: &str) -> bool {
    matches!(activity_type, "Redeem" | "Claim")
}

#[derive(Debug, Serialize)]
struct ActivityNotification {
    activity_type: String,
    market: String,
    market_slug: Option<String>,
    outcome: Option<String>,
    side: Option<String>,
    size: String,
    usdc_size: String,
    price: Option<String>,
    timestamp: i64,
    tx_hash: String,
    condition_id: Option<String>,
    asset: Option<String>,
    username: Option<String>,
}

#[derive(Debug, Clone)]
struct MarketInfo {
    title: String,
    slug: Option<String>,
}

impl ActivityNotification {
    fn from_activity(activity: &Activity) -> Self {
        let market = activity
            .title
            .clone()
            .or_else(|| activity.slug.clone())
            .unwrap_or_else(|| "Unknown market".to_string());
        let username = activity.name.clone().or_else(|| activity.pseudonym.clone());
        Self {
            activity_type: format!("{:?}", activity.activity_type),
            market,
            market_slug: activity.slug.clone(),
            outcome: activity.outcome.clone(),
            side: activity.side.as_ref().map(|side| format!("{:?}", side)),
            size: format_decimal(activity.size),
            usdc_size: format_decimal(activity.usdc_size),
            price: activity.price.map(format_decimal),
            timestamp: activity.timestamp,
            tx_hash: activity.transaction_hash.to_string(),
            condition_id: activity.condition_id.map(|id| id.to_string()),
            asset: activity.asset.map(|a| a.to_string()),
            username,
        }
    }
}

fn format_activity_message(
    wallet_address: &str,
    label: Option<&str>,
    notification: &ActivityNotification,
) -> String {
    let reward = is_reward_activity(&notification.activity_type);
    let (emoji, action) = match notification.activity_type.as_str() {
        "Buy" => ("🟢", "BUY"),
        "Sell" => ("🔴", "SELL"),
        "Trade" => match notification.side.as_deref() {
            Some("Buy") => ("🟢", "BUY"),
            Some("Sell") => ("🔴", "SELL"),
            _ => ("📊", "TRADE"),
        },
        "Redeem" | "Claim" => ("🟠", "CLOSED"),
        "Unknown(\"REFERRAL_REWARD\")" | "REFERRAL_REWARD" => ("🎁", "REFERRAL REWARD"),
        "Unknown(\"MAKER_REBATE\")" | "MAKER_REBATE" | "MakerRebate" => ("💰", "MAKER REBATE"),
        "Unknown(\"TAKER_REBATE\")" | "TAKER_REBATE" | "TakerRebate" => ("💵", "TAKER REBATE"),
        _ => ("📊", notification.activity_type.as_str()),
    };

    let name_line = match (label, &notification.username) {
        (Some(name), Some(username)) => format!(
            "\nName: {name} (<a href=\"https://polymarket.com/profile/{wallet_address}\">@{username}</a>)"
        ),
        (Some(name), None) => format!(
            "\nName: {name} (<a href=\"https://polymarket.com/profile/{wallet_address}\">@{wallet_address}</a>)"
        ),
        (None, Some(username)) => format!(
            "\nName: <a href=\"https://polymarket.com/profile/{wallet_address}\">@{username}</a>"
        ),
        (None, None) => String::new(),
    };

    let market_line = match &notification.market_slug {
        Some(slug) => format!(
            "<a href=\"https://polymarket.com/event/{slug}\">{}</a>",
            notification.market
        ),
        None => notification.market.clone(),
    };

    let outcome = notification.outcome.as_deref().unwrap_or("N/A");
    let size = Decimal::from_str(notification.size.as_str())
        .ok()
        .map(number_format::format_value)
        .unwrap_or_else(|| notification.size.clone());
    let price = notification
        .price
        .as_deref()
        .and_then(number_format::format_price_with_odds_str)
        .unwrap_or_else(|| "N/A".to_string());
    let value = Decimal::from_str(notification.usdc_size.as_str())
        .ok()
        .map(number_format::format_usd)
        .unwrap_or_else(|| format!("${}", notification.usdc_size));

    if reward {
        return format!(
            "{emoji} <b>{action}</b>\n\n\
            👛 Wallet: <code>{wallet_address}</code>{name_line}\n\
            Value: {value}",
        );
    }

    format!(
        "{emoji} <b>{action}</b>\n\n\
        👛 Wallet: <code>{wallet_address}</code>{name_line}\n\
        Market: {market_line}\n\
        Outcome: {outcome}\n\
        Size: {size}\n\
        Price: {price}\n\
        Value: {value}",
    )
}

async fn handle_ws_message(
    bot: &teloxide::prelude::Bot,
    db: &Db,
    wallet: &db::ManagedWalletWithUser,
    message: WsMessage,
    data_client: &DataClient,
    market_cache: &MarketCache,
) {
    let label = wallet
        .label
        .as_deref()
        .unwrap_or(wallet.wallet_address.as_str());

    match message {
        WsMessage::Trade(trade) => {
            let market_label = resolve_market_label(data_client, market_cache, trade.market).await;
            let mut should_send = true;
            let timestamp = trade.timestamp.or(trade.matchtime).or(trade.last_update);
            let tx_hash = trade.transaction_hash.map(|hash| hash.to_string());
            if let (Some(tx_hash), Some(timestamp)) = (tx_hash.as_deref(), timestamp) {
                match db::insert_activity_log(
                    db,
                    wallet.user_id,
                    &wallet.wallet_address,
                    "WS_TRADE",
                    None,
                    tx_hash,
                    timestamp,
                    None,
                    true,
                )
                .await
                {
                    Ok(inserted) => should_send = inserted,
                    Err(_) => should_send = true,
                }
            }

            if !should_send {
                return;
            }

            let message = format_ws_trade_message(label, &trade, &market_label);
            let _ = bot
                .send_message(teloxide::types::ChatId(wallet.chat_id), message)
                .await;
        }
        WsMessage::Order(order) => {
            let market_label = resolve_market_label(data_client, market_cache, order.market).await;
            let message = format_ws_order_message(label, &order, &market_label);
            let _ = bot
                .send_message(teloxide::types::ChatId(wallet.chat_id), message)
                .await;
        }
        _ => {}
    }
}

async fn resolve_market_label(client: &DataClient, cache: &MarketCache, market: B256) -> String {
    if let Some(info) = lookup_market_info(client, cache, market).await {
        return format_market_label(&info);
    }

    market.to_string()
}

async fn lookup_market_info(
    client: &DataClient,
    cache: &MarketCache,
    market: B256,
) -> Option<MarketInfo> {
    let key = market.to_string();
    if let Some(info) = cache.lock().await.get(&key).cloned() {
        return Some(info);
    }

    let builder = TradesRequest::builder()
        .filter(MarketFilter::markets([market]))
        .limit(1)
        .ok()?;
    let request = builder.build();
    let trades = client.trades(&request).await.ok()?;
    let trade = trades.first()?;
    let info = MarketInfo {
        title: trade.title.clone(),
        slug: Some(trade.slug.clone()),
    };

    cache.lock().await.insert(key, info.clone());
    Some(info)
}

fn format_market_label(info: &MarketInfo) -> String {
    match info.slug.as_deref() {
        Some(slug) => format!("{} ({slug})", info.title),
        None => info.title.clone(),
    }
}

fn format_ws_trade_message(label: &str, trade: &TradeMessage, market_label: &str) -> String {
    let status = format!("{:?}", trade.status);
    let side = format!("{:?}", trade.side);
    let role = trade
        .trader_side
        .as_ref()
        .map(|side| format!("{:?}", side))
        .unwrap_or_else(|| "N/A".to_string());
    let outcome = trade.outcome.as_deref().unwrap_or("N/A");
    let timestamp = trade
        .timestamp
        .or(trade.matchtime)
        .or(trade.last_update)
        .map(|value| value.to_string())
        .unwrap_or_else(|| "N/A".to_string());
    let tx_hash = trade
        .transaction_hash
        .map(|hash| hash.to_string())
        .unwrap_or_else(|| "N/A".to_string());

    format!(
        "Trade for {label}\nStatus: {status} | Side: {side} | Role: {role}\nMarket: {market}\nAsset: {asset}\nOutcome: {outcome}\nSize: {size} @ {price}\nTx: {tx_hash} | Time: {timestamp}",
        market = market_label,
        asset = trade.asset_id,
        size = format_decimal(trade.size),
        price = number_format::format_price_with_odds(trade.price),
    )
}

fn format_ws_order_message(label: &str, order: &OrderMessage, market_label: &str) -> String {
    let msg_type = order
        .msg_type
        .as_ref()
        .map(|msg_type| format!("{:?}", msg_type))
        .unwrap_or_else(|| "Update".to_string());
    let outcome = order.outcome.as_deref().unwrap_or("N/A");
    let timestamp = order
        .timestamp
        .map(|value| value.to_string())
        .unwrap_or_else(|| "N/A".to_string());
    let original_size = format_optional_decimal(order.original_size);
    let matched = format_optional_decimal(order.size_matched);

    format!(
        "Order for {label}\nType: {msg_type}\nOrder: {id}\nMarket: {market}\nAsset: {asset}\nSide: {side} | Outcome: {outcome}\nPrice: {price} | Original: {original_size} | Matched: {matched}\nTime: {timestamp}",
        id = order.id,
        market = market_label,
        asset = order.asset_id,
        side = format!("{:?}", order.side),
        price = number_format::format_price_with_odds(order.price),
    )
}

fn format_optional_decimal(value: Option<Decimal>) -> String {
    number_format::format_option_value(value)
}
