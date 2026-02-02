CREATE TABLE IF NOT EXISTS users (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  telegram_id BIGINT UNIQUE NOT NULL,
  chat_id BIGINT NOT NULL,
  current_mode TEXT CHECK(current_mode IN ('none', 'track', 'manage')) DEFAULT 'none',
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  last_active TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS tracked_wallets (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER NOT NULL,
  wallet_address TEXT NOT NULL,
  label TEXT,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  FOREIGN KEY (user_id) REFERENCES users(id),
  UNIQUE(user_id, wallet_address)
);

CREATE TABLE IF NOT EXISTS position_snapshots (
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

CREATE TABLE IF NOT EXISTS managed_wallets (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER NOT NULL,
  wallet_address TEXT NOT NULL,
  encrypted_key BLOB NOT NULL,
  nonce BLOB NOT NULL,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  FOREIGN KEY (user_id) REFERENCES users(id),
  UNIQUE(user_id, wallet_address)
);

CREATE TABLE IF NOT EXISTS activity_log (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  wallet_address TEXT NOT NULL,
  activity_type TEXT NOT NULL,
  market_slug TEXT,
  details TEXT,
  notified BOOLEAN DEFAULT FALSE,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_tracked_wallets_user_id ON tracked_wallets(user_id);
CREATE INDEX IF NOT EXISTS idx_tracked_wallets_wallet_address ON tracked_wallets(wallet_address);
CREATE INDEX IF NOT EXISTS idx_position_snapshots_wallet_address ON position_snapshots(wallet_address);
CREATE INDEX IF NOT EXISTS idx_managed_wallets_user_id ON managed_wallets(user_id);
CREATE INDEX IF NOT EXISTS idx_activity_log_wallet_address ON activity_log(wallet_address);
CREATE INDEX IF NOT EXISTS idx_activity_log_notified ON activity_log(notified);
CREATE INDEX IF NOT EXISTS idx_activity_log_created_at ON activity_log(created_at);
