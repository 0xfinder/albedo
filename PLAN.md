# Polymarket Telegram Bot Implementation Plan

## Summary
- Build a Telegram bot that tracks and manages Polymarket positions.
- Start with a minimal, working bot and iterate toward full track/manage flows.

## Goals
- Support Track and Manage modes with clean command UX.
- Persist user + wallet data in SQLite.
- Monitor positions and notify on changes.
- Securely store authenticated wallet keys.

## Non-goals (for now)
- Web UI or dashboard.
- Automated trading strategies.
- Multi-chain or non-Polymarket integrations.

## Architecture Overview
```
┌─────────────────────────────────────────────────────┐
│ Telegram Bot (Teloxide)                             │
├─────────────────────────────────────────────────────┤
│ ┌──────────────┐ ┌──────────────┐ ┌────────────┐     │
│ │ Track Mode   │ │ Manage Mode  │ │ Monitor    │     │
│ │ - Add wallet │ │ - View pos   │ │ Loop       │     │
│ │ - List       │ │ - Place order│ │ - Poll API  │     │
│ │ - Remove     │ │ - Cancel     │ │ - Notify    │     │
│ └──────────────┘ └──────────────┘ └────────────┘     │
├─────────────────────────────────────────────────────┤
│ Polymarket CLOB Client (SDK)                        │
├─────────────────────────────────────────────────────┤
│ SQLite Database                                     │
│ - users (id, chat_id, mode)                         │
│ - tracked_wallets (user_id, address, label)         │
│ - position_snapshots (wallet, market, data, time)   │
│ - managed_wallets (user_id, encrypted_key, nonce)   │
│ - activity_log (wallet, type, details)              │
└─────────────────────────────────────────────────────┘
```

## Tech Stack
### Core
- Rust
- Teloxide (Telegram framework)
- Tokio (async runtime)
- SQLite + SQLx (persistence)
- Polymarket CLOB SDK

### Dependencies (Phase 0: basic bot)
```
[dependencies]
dotenv = "0.15"
color-eyre = "0.6"
tokio = { version = "1.38", features = ["macros", "rt-multi-thread"] }
teloxide = { version = "0.13", features = ["macros"] }
```

### Dependencies (Planned Phase 1+)
```
polymarket-client-sdk = { version = "0.4", features = ["clob", "data", "ws"] }
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite", "migrate"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
thiserror = "1.0"
chrono = { version = "0.4", features = ["serde"] }
aes-gcm = "0.10"
```

## Environment Variables
- `TELEGRAM_TOKEN` (required for Phase 0)
- `DATABASE_URL` (default: `sqlite://bot.db`)
- `ENCRYPTION_KEY` (required when storing private keys)
- `POLYMARKET_API_URL` (default: `https://clob.polymarket.com`)

## Database Schema (SQLite)
```
-- Users table
CREATE TABLE users (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  telegram_id BIGINT UNIQUE NOT NULL,
  chat_id BIGINT NOT NULL,
  current_mode TEXT CHECK(current_mode IN ('none', 'track', 'manage')) DEFAULT 'none',
  pending_action TEXT,
  pending_data TEXT,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  last_active TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Wallet tracking (for track mode)
CREATE TABLE tracked_wallets (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER NOT NULL,
  wallet_address TEXT NOT NULL,
  label TEXT,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  FOREIGN KEY (user_id) REFERENCES users(id),
  UNIQUE(user_id, wallet_address)
);

-- Cached positions (for monitoring changes)
CREATE TABLE position_snapshots (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  wallet_address TEXT NOT NULL,
  market_slug TEXT,
  market_question TEXT,
  outcome TEXT,
  position_size TEXT,
  avg_price TEXT,
  total_value TEXT,
  snapshot_time TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Authenticated wallets (for manage mode)
CREATE TABLE managed_wallets (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER NOT NULL,
  wallet_address TEXT NOT NULL,
  encrypted_key BLOB NOT NULL,
  nonce BLOB NOT NULL,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  FOREIGN KEY (user_id) REFERENCES users(id),
  UNIQUE(user_id, wallet_address)
);

-- Activity log (for notifications)
CREATE TABLE activity_log (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  wallet_address TEXT NOT NULL,
  activity_type TEXT NOT NULL,
  market_slug TEXT,
  details TEXT,
  notified BOOLEAN DEFAULT FALSE,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

## Database Notes
- Enable foreign keys on every SQLite connection.
- Normalize `wallet_address` to lowercase before insert.
- Add indexes for lookup-heavy queries.

Example indexes:
```
CREATE INDEX IF NOT EXISTS idx_tracked_wallets_user_id ON tracked_wallets(user_id);
CREATE INDEX IF NOT EXISTS idx_position_snapshots_wallet_address ON position_snapshots(wallet_address);
CREATE INDEX IF NOT EXISTS idx_managed_wallets_user_id ON managed_wallets(user_id);
CREATE INDEX IF NOT EXISTS idx_activity_log_notified ON activity_log(notified);
CREATE INDEX IF NOT EXISTS idx_activity_log_created_at ON activity_log(created_at);
```

## File Structure (Target)
```
src/
├── main.rs
├── config.rs
├── db/
│   ├── mod.rs
│   ├── models.rs
│   └── migrations/
├── bot/
│   ├── mod.rs
│   ├── commands.rs
│   └── handlers.rs
├── modes/
│   ├── mod.rs
│   ├── track/
│   ├── manage/
├── polymarket/
│   ├── mod.rs
│   ├── client.rs
│   └── tracking.rs
├── monitoring/
│   ├── mod.rs
│   └── notifier.rs
└── utils/
    ├── mod.rs
    ├── crypto.rs
    └── format.rs
```

## Implementation Phases
### Phase 0: Basic Bot (now)
- Teloxide dispatcher running.
- `/start`, `/help`, `/track`, `/manage` with placeholder responses.
- No persistence or Polymarket calls yet.

### Phase 1: Foundation (Database & Basic Bot)
- Add SQLx + SQLite migrations.
- Register users on first interaction.
- Store current mode in DB.

### Phase 2: Track Mode
Commands:
- `/track add <address> [label]`
- `/track list`
- `/track remove <address>`
- `/track status`

Features:
- Store wallet addresses.
- Fetch positions (unauthenticated).
- Background monitoring task.

### Phase 3: Monitoring & Notifications
- Poll every 30-60 seconds.
- Compare snapshots, detect changes.
- Log activity and notify users.

### Phase 4: Manage Mode
Commands:
- `/manage auth <private_key> [label]`
- `/manage list`
- `/manage positions [wallet]`
- `/manage market_order <wallet> <market> <side> <amount>`
- `/manage limit_order <wallet> <market> <side> <price> <size>`
- `/manage cancel <wallet> <order_id>`
- `/manage remove <wallet>`

Security:
- Encrypt private keys with AES-GCM.
- Use `ENCRYPTION_KEY` from env.
- Never log or expose keys.

### Phase 5: Polish & Edge Cases
- API error handling and retries.
- Rate limiting.
- Help docs and graceful shutdown.

## Key Flows
### Mode Switching Flow
```
User starts bot
  ↓
Show mode selection keyboard
  ↓
User selects Track or Manage
  ↓
Persist mode in DB
  ↓
Show mode-specific commands
```

### Background Monitoring
- Spawned in `main.rs` once DB is added.
- Poll Polymarket, compare snapshots, enqueue notifications.

## Runbook
- Set `TELEGRAM_TOKEN` in `.env`.
- Run `cargo run`.

## Testing
- `cargo test` (once tests exist).
- `cargo fmt` before commits.

## Risks and Notes
- Private key handling must be airtight.
- Polymarket rate limits may require backoff.

## Open Questions
- How long should we retain activity history?
- Do we need per-user polling intervals?
