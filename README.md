# Albedo

Personal Telegram bot because the Polymarket UI sucks.

## Features

- **Track wallets**: Monitor any wallet's activity and position changes
- **Manage wallets**: Store encrypted private keys to place orders directly from Telegram
- Real-time notifications for trades and position updates

## Requirements

- Rust (1.70+)
- SQLite

## Setup

1. Clone the repository:
   ```bash
   git clone https://github.com/0xfinder/albedo.git
   cd albedo
   ```

2. Create a `.env` file:
   ```bash
   cp .env.example .env
   ```

3. Configure environment variables in `.env`:
   ```env
   # Required
   TELEGRAM_TOKEN=your_telegram_bot_token

   # Optional - defaults to sqlite://bot.db
   DATABASE_URL=sqlite://bot.db

   # Optional - polling interval in seconds (default: 1)
   POLYMARKET_DATA_POLL_SECONDS=1

   # Optional - for encrypting managed wallet private keys
   # Generate with: openssl rand -hex 32
   ENCRYPTION_KEY=your_64_char_hex_key
   ```

4. Build and run:
   ```bash
   cargo build --release
   cargo run --release
   ```

## Usage

Start a conversation with your bot on Telegram and use `/start` to see the menu.

### Track Mode
- Add wallet addresses to monitor
- Receive notifications when tracked wallets make trades

### Manage Mode
- Authenticate with your private key (encrypted and stored locally)
- View positions and place market/limit orders
- Cancel orders directly from Telegram

## Testing

Run the test suite:
```bash
cargo test
```
