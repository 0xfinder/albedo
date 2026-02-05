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
| `Redeem` | Redeeming outcome tokens after market resolution (same as Claim) |
| `Claim` | Alias for Redeem |
| `Reward` | Receiving a reward (e.g., liquidity mining) |
| `Conversion` | Converting between token types |
| `Yield` | Yield earnings |
| `MakerRebate` | Fee rebate for providing liquidity |

### Redeem/Claim Pricing

When a market resolves, positions are redeemed at their payout value:

| Outcome | Price |
|---------|-------|
| Win | $1.00 per token |
| Loss | $0.00 per token |
| Partial (rare) | Proportional (e.g., 70/30 split → $0.70 / $0.30) |

The `usdc_size` field shows the actual payout received. For losing positions, this will be $0.00.

## Request Limits

| Endpoint | Max Limit | Default |
|----------|-----------|---------|
| `/activity` | 500 | 100 |
| `/positions` | 500 | 100 |
| `/trades` | 10000 | 100 |
| `/holders` | 20 | 20 |

## Account Setup for API Trading

### Wallet Types

| Type | Signature Type | Description |
|------|----------------|-------------|
| EOA (MetaMask, hardware) | 0 | Standard externally owned account |
| Magic/Email (Google login) | 1 | Email wallet via Magic Link |
| Browser proxy | 2 | Browser extension proxy wallets |

### Google/Email Account Setup

For accounts created via Google/email login (Magic wallet):

1. **Export private key** - https://reveal.magic.link/polymarket
2. **Funder address** - Your Polymarket profile address (holds USDC, different from signing wallet)
3. **API credentials** - Derived automatically via `create_or_derive_api_key()`
4. **Signature type** - Must be set to `1` for Magic wallets
5. **Token allowances** - Set automatically for Magic wallets

### EOA Account Setup (MetaMask, etc.)

1. **Private key** - Export from wallet
2. **Funder address** - Same as wallet address (no proxy)
3. **API credentials** - Derived automatically
4. **Signature type** - Set to `0`
5. **Token allowances** - Must be set manually before trading

### Funder vs Signing Wallet

For Magic/email wallets, the signing key is different from the address holding funds:

- **Signing wallet**: Derived from exported private key, used to sign transactions
- **Funder wallet**: Your Polymarket profile address, holds your USDC deposits

The API client needs both to function correctly.

### Auto-Detecting Wallet Type

The wallet type (EOA vs Magic) can be auto-detected:

1. **Derive both addresses** from the private key:
   - EOA address = signer address directly
   - Proxy address = `derive_proxy_wallet(signer_address, POLYGON)` (CREATE2)

2. **Check which has activity/positions** via the Data API:
   - Query positions for proxy address first (more common for new users)
   - If positions exist → `SignatureType::Proxy`
   - Otherwise fallback to `SignatureType::Eoa`

```rust
use polymarket_client_sdk::{derive_proxy_wallet, POLYGON};

let eoa_address = signer.address();
let proxy_address = derive_proxy_wallet(eoa_address, POLYGON);

// Check positions at proxy address first
let proxy_positions = data_client.positions(proxy_address).await;
let signature_type = if !proxy_positions.unwrap_or_default().is_empty() {
    SignatureType::Proxy
} else {
    SignatureType::Eoa
};
```

### Proxy Wallet Derivation

The SDK provides `derive_proxy_wallet` to compute the deterministic proxy address:

```rust
use polymarket_client_sdk::{derive_proxy_wallet, POLYGON};

let proxy_address = derive_proxy_wallet(eoa_address, POLYGON);
// Returns Option<Address> - None if chain doesn't support proxies
```

This uses CREATE2 with:
- Factory: `0xaB45c5A4B0c941a2F231C04C3f49182e1A254052` (Polygon)
- Salt: `keccak256(eoa_address)`
- Init code hash: EIP-1167 minimal proxy
