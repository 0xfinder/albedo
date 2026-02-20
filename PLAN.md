# Copy Trade Feature

## Overview

Add a "Copy Trade" inline button to activity notification messages, allowing users to quickly replicate (or counter) a tracked wallet's trade using their managed wallet.

## Button Placement

Activity notifications for tradeable events (Buy, Sell, Trade) that include an asset (token_id) show two buttons:

```
[📊 Show Positions] [📋 Copy Trade]
```

Non-tradeable activity types (Redeem, Claim, Reward, Merge, Split, Conversion, Yield, MakerRebate) only show "Show Positions."

## Flow

### Step 1: Tap "📋 Copy Trade"

Bot checks whether the user has a managed wallet configured. If not, replies:
> Set up a managed wallet first via /manage.

If yes, bot sends an order preview with defaults pulled from the activity:

```
📋 Copy Trade

Market: LoL: Team A vs Team B - Game 2 Winner
Outcome: Team A
Side: BUY
Price: 47¢ (limit)
Shares: 100
Est. Cost: $47.00

[✅ Confirm] [❌ Cancel]
[💰 Price] [📊 Size]
[↕️ Flip Side] [🔄 Market Order]
```

Defaults:
- Side: same as the activity (copy trade).
- Price: same as the activity price.
- Shares: same as the activity size.
- Order type: limit.

### Step 2: Editing

Each edit button enters a text-input mode using the existing `pending_state` mechanism.

| Button | Behavior |
|--------|----------|
| **💰 Price** | Bot asks for a new price. User sends a number. Preview re-renders in place. |
| **📊 Size** | Bot asks for a new share count. User sends a number. Preview re-renders in place. |
| **↕️ Flip Side** | Toggles between BUY and SELL instantly. Preview re-renders. Button label stays the same (it's a toggle). |
| **🔄 Market Order** | Toggles between limit and market order. In market mode, price row shows "at market" and the button label changes to "🔄 Limit Order." |

Re-renders use Telegram's `edit_message_text` to update the preview in place rather than sending new messages.

### Step 3: Confirm

Tapping "✅ Confirm" places the order via the CLOB client using the managed wallet's encrypted credentials, following the same code path as the existing `manage:market_order` / `manage:limit_order` handlers.

Bot sends a result message (success with order ID, or error details).

### Cancel

"❌ Cancel" deletes the preview message (or replaces it with "Cancelled.") and clears pending state.

## Data Storage

### Copy trade state table

Store order parameters so callbacks can reference them by short ID (Telegram limits callback data to 64 bytes).

```sql
CREATE TABLE IF NOT EXISTS copy_trade_state (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    token_id TEXT NOT NULL,
    side TEXT NOT NULL,
    price TEXT NOT NULL,
    size TEXT NOT NULL,
    order_type TEXT NOT NULL DEFAULT 'limit',
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users(id)
);
```

### Callback data format

- `ct:{id}` — open the copy trade preview for this state row.
- `ct_confirm:{id}` — place the order.
- `ct_cancel:{id}` — cancel.
- `ct_flip:{id}` — toggle side.
- `ct_market:{id}` — toggle limit/market.
- `ct_price:{id}` — enter price edit mode.
- `ct_size:{id}` — enter size edit mode.

### Pending state for text input

When editing price or size, set `pending_action` to `copy_trade_edit_price` or `copy_trade_edit_size` and `pending_data` to the state row ID. When the user sends a number, update the row and re-render the preview.

## Activity Notification Changes

### Add asset to callback_data

The existing `callback_data` table stores `wallet_address` and `condition_id`. For the copy trade button, we also need the `token_id` (asset), `side`, `price`, and `size` from the activity. Rather than overloading `callback_data`, the copy trade button creates a row in `copy_trade_state` when clicked, using values from the activity notification.

To make this work, the `callback_data` table needs additional fields from the activity:

```sql
ALTER TABLE callback_data ADD COLUMN token_id TEXT;
ALTER TABLE callback_data ADD COLUMN side TEXT;
ALTER TABLE callback_data ADD COLUMN price TEXT;
ALTER TABLE callback_data ADD COLUMN size TEXT;
```

The "Copy Trade" button callback is `ct:{callback_data_id}`. When clicked, the handler reads these fields from `callback_data`, creates a `copy_trade_state` row for the user, and renders the preview.

### Which activities show the button

Only show "📋 Copy Trade" when:
- `activity_type` is Buy, Sell, or Trade.
- `asset` (token_id) is present on the activity.

## UX Improvements

1. **Current market price**: fetch and display alongside the activity price in the preview so the user can see if the price has moved.
2. **USDC balance check**: before confirming a buy, show available USDC balance and warn if insufficient.
3. **In-place editing**: all edits re-render the same message via `edit_message_text` to keep the chat clean.
4. **Flip Side**: toggles BUY↔SELL on the same outcome token. Useful for counter-trading (e.g., if a tracked wallet buys Team A at 47¢, you can sell at the same price if you disagree).

## Edge Cases

- **No managed wallet**: prompt user to set one up.
- **Market resolved/closed**: CLOB will reject the order; surface the error clearly.
- **Activity missing asset**: don't show the Copy Trade button.
- **Stale price**: if the user waits a long time before confirming, the limit order may not fill. Market order avoids this but has slippage risk.
