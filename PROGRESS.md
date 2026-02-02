# Progress

## Current Status
- Telegram bot runs with inline menus.
- Track flow supports add/remove/list/status with label prompts.
- SQLite database with migrations and tracking state fields.
- Data API polling for tracked wallets (activity + positions).
- WS user events scaffold for managed wallets (env-based for now).

## What Works Now
- `/start` clears reply keyboard and shows Track/Manage menu.
- Track menu actions:
  - Add address -> prompt for label -> stores wallet.
  - Remove address -> deletes wallet.
  - View all -> shows tracked wallets + labels.
  - Status -> shows tracked count.
- Polling task:
  - Fetches `/activity` and `/positions` per tracked wallet.
  - Sends Telegram notifications for new activity.
  - Sends a simple notification on position changes.
- WS user events:
  - Connects if credentials are set, but does not yet emit messages.

## What’s Left
- Manage mode:
  - Authenticate wallet keys, store encrypted credentials.
  - Map managed wallets to WS user events.
  - Manage commands (list, positions, place/cancel orders).
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
