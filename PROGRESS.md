# Progress

## Current Status
- Telegram bot runs with inline Track/Manage menus.
- Track flow supports add/remove/list/status with label prompts.
- Manage flow supports auth/list/positions/market+limit orders/cancel/remove with encrypted keys.
- SQLite database with migrations and tracking state fields.
- Data API polling for tracked wallets (activity + positions).
- WS user events connect for managed wallets (no notifications yet).

## What Works Now
- `/start` clears reply keyboard and shows Track/Manage menu.
- Track menu actions:
  - Add address -> prompt for label -> stores wallet.
  - Remove address -> deletes wallet.
  - View all -> shows tracked wallets + labels.
  - Status -> shows tracked count.
- Manage menu actions:
  - Auth wallet -> stores encrypted private key + optional label.
  - List -> shows managed wallets + labels.
  - Positions -> fetches open positions for a managed wallet.
  - Market/limit orders -> submits orders for managed wallets.
  - Cancel order -> cancels order id for a managed wallet.
  - Remove wallet -> deletes managed wallet.
- Polling task:
  - Fetches `/activity` and `/positions` per tracked wallet.
  - Sends Telegram notifications for new activity.
  - Sends a simple notification on position changes.
- WS user events:
- Connects for managed wallets, but does not yet emit messages.

## What’s Left
- Monitoring pipeline:
  - Normalize activity messages into structured notifications.
  - Deduplicate activity by tx hash + timestamp.
  - Add reconnect/backoff for WS.
- Track mode improvements:
  - Inline remove buttons in list.
  - Better status (last poll time, WS state).
- Data polling controls:
  - Rate limiting/backoff.
  - Optional polling disable per user.

## Notes
- WS user events only work for managed wallets with API credentials.
- Tracking arbitrary wallets uses Data API polling.
- Default poll interval is 1s; may need to tune to avoid rate limits.
- DB migrations add pending action/data plus tracking hash fields.
