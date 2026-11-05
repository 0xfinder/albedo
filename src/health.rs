//! In-memory polling health and rate-limited Telegram incident notifications.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use teloxide::prelude::*;
use tokio::sync::Mutex;

use crate::{db, state::AppState};

const FAILURE_DELAY: Duration = Duration::from_secs(120);
const REMINDER_DELAY: Duration = Duration::from_secs(30 * 60);
pub(crate) type SharedHealth = Arc<Mutex<Health>>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum Key {
    Fetch(String, String),
    Delivery(i64),
    Database,
}

#[derive(Debug)]
struct Check {
    created: Instant,
    last_success: Option<Instant>,
    failure: Option<(Instant, String)>,
    freshness_grace: Duration,
}

impl Check {
    fn failing_since(&self, key: &Key) -> Option<Instant> {
        self.failure.as_ref().map(|(since, _)| *since).or_else(|| {
            matches!(key, Key::Fetch(..)).then_some(self.last_success.unwrap_or(self.created))
        })
    }

    fn overdue(&self, key: &Key, now: Instant) -> bool {
        let threshold = if self.failure.is_some() {
            FAILURE_DELAY
        } else {
            self.freshness_grace
        };
        self.failing_since(key)
            .is_some_and(|since| now.duration_since(since) >= threshold)
    }

    fn describe(&self, key: &Key, now: Instant) -> String {
        let success = self.last_success.map_or_else(
            || "never since restart".into(),
            |time| format!("{}s ago", now.duration_since(time).as_secs()),
        );
        let status = self.failure.as_ref().map_or_else(
            || {
                if self.overdue(key, now) {
                    "STALE: polling has stopped making progress".into()
                } else if self.last_success.is_some() {
                    "OK".into()
                } else {
                    "waiting for first result".into()
                }
            },
            |(_, error)| format!("FAILING: {error}"),
        );
        format!("{status}; last success: {success}")
    }
}

#[derive(Debug, Default)]
pub(crate) struct Health {
    checks: HashMap<Key, Check>,
    incidents: HashMap<i64, Instant>,
    poll_interval: Duration,
}

struct Alert {
    text: String,
    failing: bool,
}

impl Health {
    pub(crate) fn set_poll_interval(&mut self, interval: Duration) {
        self.poll_interval = interval;
    }

    fn register(&mut self, key: Key, now: Instant) -> &mut Check {
        self.checks.entry(key).or_insert(Check {
            created: now,
            last_success: None,
            failure: None,
            freshness_grace: FAILURE_DELAY.max(self.poll_interval.saturating_mul(3)),
        })
    }

    pub(crate) fn record(&mut self, key: Key, error: Option<String>, now: Instant) {
        let check = self.register(key, now);
        if let Some(error) = error {
            let since = check.failure.as_ref().map_or(now, |(since, _)| *since);
            check.failure = Some((since, error.chars().take(240).collect()));
        } else {
            check.last_success = Some(now);
            check.failure = None;
        }
    }

    pub(crate) fn report(
        &self,
        wallets: &[db::TrackedWalletWithUser],
        chat_id: i64,
        now: Instant,
    ) -> String {
        let mut keys = keys_for(wallets, chat_id);
        keys.sort_by_key(|(_, key)| {
            self.checks
                .get(key)
                .is_none_or(|check| check.failure.is_none() && !check.overdue(key, now))
        });
        let mut text = String::from("Monitoring health (since restart)\n");
        if wallets.is_empty() {
            text.push_str("No tracked wallets.\n");
        }
        for (label, key) in keys {
            let status = self.checks.get(&key).map_or_else(
                || "no attempts since restart".into(),
                |check| check.describe(&key, now),
            );
            let line = format!("\n{label}: {status}\n");
            if text.encode_utf16().count() + line.encode_utf16().count() > 3400 {
                text.push_str("\nMore wallets omitted.");
                break;
            }
            text.push_str(&line);
        }
        text
    }

    fn notification(
        &self,
        user_id: i64,
        wallets: &[db::TrackedWalletWithUser],
        chat_id: i64,
        now: Instant,
    ) -> Option<Alert> {
        let keys = keys_for(wallets, chat_id);
        let failing = keys
            .iter()
            .any(|(_, key)| self.checks.get(key).is_some_and(|c| c.overdue(key, now)));
        let previous = self.incidents.get(&user_id);
        if failing {
            if previous.is_some_and(|time| now.duration_since(*time) < REMINDER_DELAY) {
                return None;
            }
            Some(Alert {
                failing: true,
                text: format!(
                    "⚠️ Albedo monitoring needs attention (failure or no progress for at least 2 minutes).\n{}",
                    self.report(wallets, chat_id, now)
                ),
            })
        } else if previous.is_some()
            && keys.iter().all(|(_, key)| {
                self.checks
                    .get(key)
                    .is_none_or(|c| c.failure.is_none() && !c.overdue(key, now))
            })
        {
            Some(Alert { failing: false, text: "✅ Albedo monitoring recovered. All currently tracked checks are healthy. Use /health for details.".into() })
        } else {
            None
        }
    }
}

fn keys_for(wallets: &[db::TrackedWalletWithUser], chat_id: i64) -> Vec<(String, Key)> {
    let mut keys = Vec::new();
    for wallet in wallets {
        for endpoint in ["activity", "positions"] {
            keys.push((
                format!("{} / {endpoint}", wallet.wallet_address),
                Key::Fetch(wallet.wallet_address.to_ascii_lowercase(), endpoint.into()),
            ));
        }
    }
    keys.push(("Telegram activity delivery".into(), Key::Delivery(chat_id)));
    keys.push(("Wallet database".into(), Key::Database));
    keys
}

pub(crate) fn spawn(state: Arc<AppState>) {
    tokio::spawn(async move {
        if state.config.data_poll_interval.is_zero() {
            return;
        }
        let mut interval = tokio::time::interval(Duration::from_secs(15));
        let mut cached_wallets = Vec::new();
        loop {
            interval.tick().await;
            let wallets = match db::list_tracked_wallets_with_users(&state.db).await {
                Ok(wallets) => {
                    state.data_client.health.lock().await.record(
                        Key::Database,
                        None,
                        Instant::now(),
                    );
                    cached_wallets.clone_from(&wallets);
                    wallets
                }
                Err(error) => {
                    tracing::warn!(%error, "health check could not read wallets");
                    state.data_client.health.lock().await.record(
                        Key::Database,
                        Some(error.to_string()),
                        Instant::now(),
                    );
                    cached_wallets.clone()
                }
            };
            let mut users = BTreeMap::<i64, Vec<db::TrackedWalletWithUser>>::new();
            for wallet in wallets {
                if state
                    .config
                    .allowed_telegram_ids
                    .is_allowed(wallet.telegram_id)
                {
                    users.entry(wallet.user_id).or_default().push(wallet);
                }
            }
            let now = Instant::now();
            {
                let active: HashSet<_> = users
                    .values()
                    .flat_map(|wallets| {
                        keys_for(wallets, wallets[0].chat_id)
                            .into_iter()
                            .map(|(_, key)| key)
                    })
                    .chain([Key::Database])
                    .collect();
                let mut health = state.data_client.health.lock().await;
                health.checks.retain(|key, _| active.contains(key));
                health.incidents.retain(|user, _| users.contains_key(user));
            }
            for (user_id, wallets) in users {
                let chat_id = wallets[0].chat_id;
                let message = {
                    let mut health = state.data_client.health.lock().await;
                    for (_, key) in keys_for(&wallets, chat_id) {
                        health.register(key, now);
                    }
                    health.notification(user_id, &wallets, chat_id, now)
                };
                let Some(message) = message else {
                    continue;
                };
                match state.bot.send_message(ChatId(chat_id), message.text).await {
                    Ok(_) => {
                        let mut health = state.data_client.health.lock().await;
                        // Only successful alerts advance the reminder/recovery state.
                        if message.failing {
                            health.incidents.insert(user_id, Instant::now());
                        } else {
                            health.incidents.remove(&user_id);
                        }
                    }
                    Err(error) => tracing::warn!(%error, user_id, "health alert delivery failed"),
                }
            }
        }
    });
}

pub(crate) async fn send_report(
    state: &AppState,
    chat_id: ChatId,
    user_id: i64,
) -> ResponseResult<()> {
    let message = if state.config.data_poll_interval.is_zero() {
        "Monitoring is disabled (POLYMARKET_DATA_POLL_SECONDS=0).".into()
    } else {
        match db::list_tracked_wallets_with_users(&state.db).await {
            Ok(wallets) => {
                let own_wallets: Vec<_> = wallets
                    .into_iter()
                    .filter(|wallet| wallet.user_id == user_id)
                    .collect();
                state.data_client.health.lock().await.report(
                    &own_wallets,
                    chat_id.0,
                    Instant::now(),
                )
            }
            Err(error) => format!("Monitoring health unavailable: database read failed: {error}"),
        }
    };
    state.bot.send_message(chat_id, message).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wallet() -> db::TrackedWalletWithUser {
        db::TrackedWalletWithUser {
            user_id: 1,
            chat_id: 10,
            telegram_id: 10,
            wallet_address: "0xabc".into(),
            label: None,
            last_activity_hash: None,
            last_positions_hash: None,
        }
    }

    fn healthy(health: &mut Health, wallets: &[db::TrackedWalletWithUser], now: Instant) {
        for (_, key) in keys_for(wallets, 10) {
            health.record(key, None, now);
        }
    }

    #[test]
    fn sustained_failure_alerts_reminds_and_recovers_once() {
        let now = Instant::now();
        let wallets = [wallet()];
        let mut health = Health::default();
        healthy(&mut health, &wallets, now);
        let key = Key::Fetch("0xabc".into(), "activity".into());
        health.record(key.clone(), Some("HTTP 410 Gone".into()), now);
        let later = now + FAILURE_DELAY;
        health.record(key.clone(), Some("HTTP 410 Gone".into()), later);
        assert!(
            health
                .notification(
                    1,
                    &wallets,
                    10,
                    later.checked_sub(Duration::from_secs(1)).unwrap()
                )
                .is_none()
        );
        let alert = health.notification(1, &wallets, 10, later).unwrap();
        assert!(alert.failing);
        assert!(alert.text.contains("activity: FAILING: HTTP 410 Gone"));
        assert!(alert.text.contains("last success:"));
        // An unsuccessful send leaves the alert eligible for retry.
        assert!(health.notification(1, &wallets, 10, later).is_some());
        health.incidents.insert(1, later);
        assert!(
            health
                .notification(1, &wallets, 10, later + Duration::from_secs(15))
                .is_none()
        );
        assert!(
            health
                .notification(1, &wallets, 10, later + REMINDER_DELAY)
                .unwrap()
                .failing
        );
        healthy(&mut health, &wallets, later + REMINDER_DELAY);
        assert!(
            !health
                .notification(1, &wallets, 10, later + REMINDER_DELAY)
                .unwrap()
                .failing
        );
        health.incidents.remove(&1);
        assert!(
            health
                .notification(1, &wallets, 10, later + REMINDER_DELAY)
                .is_none()
        );
    }

    #[test]
    fn quiet_wallet_is_healthy_but_stalled_poller_alerts() {
        let now = Instant::now();
        let wallets = [wallet()];
        let mut health = Health::default();
        // No trades to deliver is normal; successful empty fetches count as progress.
        for (_, key) in keys_for(&wallets, 10) {
            health.register(key, now);
        }
        for step in 1..5 {
            let time = now + Duration::from_secs(step * 60);
            for endpoint in ["activity", "positions"] {
                health.record(Key::Fetch("0xabc".into(), endpoint.into()), None, time);
            }
            assert!(health.notification(1, &wallets, 10, time).is_none());
        }
        let alert = health
            .notification(1, &wallets, 10, now + Duration::from_secs(360))
            .unwrap();
        assert!(alert.text.contains("STALE"));
    }

    #[test]
    fn transient_failure_does_not_alert_and_delivery_failure_does() {
        let now = Instant::now();
        let wallets = [wallet()];
        let mut health = Health::default();
        healthy(&mut health, &wallets, now);
        health.record(Key::Delivery(10), Some("Telegram unavailable".into()), now);
        healthy(&mut health, &wallets, now + Duration::from_secs(30));
        assert!(
            health
                .notification(1, &wallets, 10, now + Duration::from_secs(30))
                .is_none()
        );
        health.record(Key::Delivery(10), Some("Telegram unavailable".into()), now);
        for endpoint in ["activity", "positions"] {
            health.record(
                Key::Fetch("0xabc".into(), endpoint.into()),
                None,
                now + FAILURE_DELAY,
            );
        }
        assert!(
            health
                .notification(1, &wallets, 10, now + FAILURE_DELAY)
                .unwrap()
                .text
                .contains("Telegram unavailable")
        );
    }

    #[test]
    fn one_healthy_wallet_does_not_hide_another_failure() {
        let now = Instant::now();
        let mut second = wallet();
        second.wallet_address = "0xdef".into();
        let wallets = [wallet(), second];
        let mut health = Health::default();
        healthy(&mut health, &wallets, now);
        health.record(
            Key::Fetch("0xabc".into(), "activity".into()),
            Some("HTTP 410".into()),
            now,
        );
        health.record(Key::Delivery(10), Some("send failed".into()), now);
        let later = now + FAILURE_DELAY;
        for endpoint in ["activity", "positions"] {
            health.record(Key::Fetch("0xdef".into(), endpoint.into()), None, later);
        }
        assert!(health.notification(1, &wallets, 10, later).unwrap().failing);
        health.incidents.insert(1, later);
        for wallet in &wallets {
            for endpoint in ["activity", "positions"] {
                health.record(
                    Key::Fetch(wallet.wallet_address.clone(), endpoint.into()),
                    None,
                    later,
                );
            }
        }
        // Fetch recovery alone must not claim that failed delivery recovered.
        assert!(health.notification(1, &wallets, 10, later).is_none());
        health.record(Key::Delivery(10), None, later);
        assert!(!health.notification(1, &wallets, 10, later).unwrap().failing);
    }

    #[test]
    fn reports_only_requested_wallets_and_bounds_message_size() {
        let now = Instant::now();
        let mut health = Health::default();
        health.record(
            Key::Fetch("private".into(), "activity".into()),
            Some("other user's error".into()),
            now,
        );
        let wallets: Vec<_> = (0..100)
            .map(|n| {
                let mut wallet = wallet();
                wallet.wallet_address = format!("wallet{n}");
                wallet
            })
            .collect();
        let report = health.report(&wallets, 10, now);
        assert!(!report.contains("other user's error"));
        assert!(report.encode_utf16().count() < 3500);
        assert!(report.contains("omitted"));
    }

    #[test]
    fn slow_poll_interval_does_not_raise_false_staleness_alarm() {
        let now = Instant::now();
        let wallets = [wallet()];
        let mut health = Health::default();
        health.set_poll_interval(Duration::from_secs(300));
        healthy(&mut health, &wallets, now);
        assert!(
            health
                .notification(1, &wallets, 10, now + Duration::from_secs(600))
                .is_none()
        );
    }
}
