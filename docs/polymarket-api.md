# Polymarket Data API Response Types

## Positions Endpoint

The `/positions` endpoint returns the current open positions for a wallet.

| Field | Type | Description |
|-------|------|-------------|
| `proxy_wallet` | Address | User's proxy wallet address |
| `asset` | U256 | Outcome token asset identifier |
| `condition_id` | B256 | Market condition ID (unique market identifier) |
| `size` | Decimal | Number of outcome tokens held |
| `avg_price` | Decimal | Average entry price for the position |
| `initial_value` | Decimal | Initial value (cost basis) of the position |
| `current_value` | Decimal | Current market value of the position |
| `cash_pnl` | Decimal | Unrealized cash profit/loss |
| `percent_pnl` | Decimal | Unrealized percentage profit/loss |
| `total_bought` | Decimal | Total amount bought (cumulative) |
| `realized_pnl` | Decimal | Realized profit/loss from closed portions |
| `percent_realized_pnl` | Decimal | Realized percentage profit/loss |
| `cur_price` | Decimal | Current market price of the outcome |
| `redeemable` | bool | Whether the position can be redeemed (market resolved) |
| `mergeable` | bool | Whether the position can be merged with opposite outcome |
| `title` | String | Market title/question |
| `slug` | String | Market URL slug |
| `icon` | String | Market icon URL |
| `event_slug` | String | Parent event URL slug |
| `event_id` | Option\<String\> | Parent event ID |
| `outcome` | String | Outcome name (e.g., "Yes", "No", candidate name) |
| `outcome_index` | i32 | Outcome index within the market (0 or 1 for binary) |
| `opposite_outcome` | String | Name of the opposite outcome |
| `opposite_asset` | U256 | Asset identifier of the opposite outcome |
| `end_date` | NaiveDate | Market end/resolution date |
| `negative_risk` | bool | Whether this is a negative risk market |

## Activity Endpoint

The `/activity` endpoint returns on-chain activity history for a wallet.

| Field | Type | Description |
|-------|------|-------------|
| `proxy_wallet` | Address | User's proxy wallet address |
| `timestamp` | i64 | Unix timestamp when the activity occurred |
| `condition_id` | Option\<B256\> | Market condition ID (optional for some activity types) |
| `activity_type` | ActivityType | Type of activity (see below) |
| `size` | Decimal | Number of tokens involved in the activity |
| `usdc_size` | Decimal | USDC value of the activity |
| `transaction_hash` | B256 | On-chain transaction hash |
| `price` | Option\<Decimal\> | Price per token (for trades) |
| `asset` | Option\<U256\> | Outcome token asset identifier |
| `side` | Option\<Side\> | Trade side: BUY or SELL (for trades only) |
| `outcome_index` | Option\<i32\> | Outcome index (for trades) |
| `title` | Option\<String\> | Market title/question |
| `slug` | Option\<String\> | Market URL slug |
| `icon` | Option\<String\> | Market icon URL |
| `event_slug` | Option\<String\> | Parent event URL slug |
| `outcome` | Option\<String\> | Outcome name |
| `name` | Option\<String\> | User's display name (if public) |
| `pseudonym` | Option\<String\> | User's pseudonym (if set) |
| `bio` | Option\<String\> | User's bio (if public) |
| `profile_image` | Option\<String\> | User's profile image URL |
| `profile_image_optimized` | Option\<String\> | User's optimized profile image URL |

## Activity Types

| Type | Description |
|------|-------------|
| `Trade` | A trade (buy or sell) of outcome tokens |
| `Split` | Splitting collateral into outcome token sets |
| `Merge` | Merging outcome token sets back into collateral |
| `Redeem` | Redeeming winning outcome tokens after market resolution |
| `Reward` | Receiving a reward (e.g., liquidity mining) |
| `Conversion` | Converting between token types |
| `Yield` | Yield earnings |
| `MakerRebate` | Fee rebate for providing liquidity |

## Request Limits

| Endpoint | Max Limit | Default |
|----------|-----------|---------|
| `/activity` | 500 | 100 |
| `/positions` | 500 | 100 |
| `/trades` | 10000 | 100 |
| `/holders` | 20 | 20 |
