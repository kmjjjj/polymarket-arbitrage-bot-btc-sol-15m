# Polymarket Arbitrage Bot

A Rust-based arbitrage bot for Polymarket that monitors SOL and BTC 15-minute price prediction markets and executes trades when arbitrage opportunities are detected.

## How It Works

The bot continuously monitors two markets:
- **SOL 15-minute price change prediction market**
- **BTC 15-minute price change prediction market**

### Arbitrage Strategy

The bot looks for opportunities where the sum of two complementary tokens (one from each market) is less than $1.00.

**Example:**
- SOL Up token: $0.47
- BTC Down token: $0.40
- Total cost: $0.87
- Expected profit: $0.13 (when market closes, one token will be worth $1.00)

When such an opportunity is detected, the bot:
1. Purchases the Up token in the SOL market
2. Purchases the Down token in the BTC market
3. Waits for market resolution to realize profit

### Results

Here's an example of an arbitrage trade executed by the bot:

![Arbitrage Trade Results](docs/arb-screenshot.png)

### Architecture

- **API Client** (`api.rs`): Handles communication with Polymarket's Gamma API and CLOB API
- **Market Monitor** (`monitor.rs`): Continuously fetches market data for both SOL and BTC markets
- **Arbitrage Detector** (`arbitrage.rs`): Analyzes prices and identifies arbitrage opportunities
- **Trader** (`trader.rs`): Executes trades in simulation or production mode

## Setup

1. Install Rust (if not already installed):
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. Build the project:
   ```bash
   cargo build --release
   ```

3. Configure the bot:
   - Edit `config.json` (created on first run) or use command-line arguments
   - Set `sol_condition_id` and `btc_condition_id` if you know them
   - Otherwise, the bot will attempt to discover them automatically

## Usage

### Simulation Mode (Default)
Test the bot without executing real trades:
```bash
cargo run -- --simulation
```

### Production Mode
Execute real trades (requires API key):
```bash
cargo run -- --no-simulation
```

### Configuration Options

- `--simulation` / `--no-simulation`: Toggle simulation mode
- `--config <path>`: Specify config file path (default: `config.json`)

### Configuration File

The bot creates a `config.json` file on first run with the following structure:

```json
{
  "polymarket": {
    "gamma_api_url": "https://gamma-api.polymarket.com",
    "clob_api_url": "https://clob.polymarket.com",
    "ws_url": "wss://clob-ws.polymarket.com",
    "api_key": null
  },
  "trading": {
    "min_profit_threshold": 0.01,
    "max_position_size": 100.0,
    "sol_condition_id": null,
    "btc_condition_id": null,
    "check_interval_ms": 1000
  }
}
```

**Important Settings:**
- `min_profit_threshold`: Minimum profit (in dollars) required to execute a trade
- `max_position_size`: Maximum amount to invest per trade
- `check_interval_ms`: How often to check for opportunities (in milliseconds)
- `api_key`: Your Polymarket API key (required for production mode)

## How the Bot Detects Opportunities

1. **Market Discovery**: The bot searches for active SOL and BTC 15-minute markets using Polymarket's Gamma API
2. **Price Monitoring**: Continuously fetches order book data to get current ask prices for Up/Down tokens
3. **Arbitrage Calculation**: For each combination (SOL Up + BTC Down, SOL Down + BTC Up), calculates total cost
4. **Opportunity Detection**: If total cost < $1.00 and profit >= `min_profit_threshold`, executes trade
5. **Trade Execution**: Places simultaneous buy orders for both tokens

## Notes

- The bot runs continuously until stopped (Ctrl+C)
- In simulation mode, all trades are logged but not executed
- The bot automatically discovers condition IDs if not provided in config
- Make sure you have sufficient balance and API permissions for production trading

