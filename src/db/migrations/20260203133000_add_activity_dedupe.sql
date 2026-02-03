ALTER TABLE activity_log ADD COLUMN user_id INTEGER;
ALTER TABLE activity_log ADD COLUMN transaction_hash TEXT;
ALTER TABLE activity_log ADD COLUMN activity_timestamp INTEGER;

CREATE INDEX IF NOT EXISTS idx_activity_log_user_id ON activity_log(user_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_activity_log_dedupe
  ON activity_log(user_id, wallet_address, transaction_hash, activity_timestamp)
  WHERE user_id IS NOT NULL AND transaction_hash IS NOT NULL AND activity_timestamp IS NOT NULL;
