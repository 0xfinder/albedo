DELETE FROM managed_wallets
WHERE id NOT IN (
  SELECT MAX(id) FROM managed_wallets GROUP BY user_id
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_managed_wallets_user_id_unique ON managed_wallets(user_id);
