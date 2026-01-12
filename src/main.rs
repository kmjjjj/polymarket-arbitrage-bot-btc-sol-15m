mod api;
mod arbitrage;
mod config;
mod models;
mod monitor;
mod trader;

use anyhow::{Context, Result};
use clap::Parser;
use config::{Args, Config};
use log::{info, warn};
use std::sync::Arc;

use api::PolymarketApi;
use arbitrage::ArbitrageDetector;
use monitor::MarketMonitor;
use trader::Trader;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    let args = Args::parse();
    let config = Config::load(&args.config)?;

    info!("🚀 Starting Polymarket Arbitrage Bot");
    info!("Mode: {}", if args.simulation { "SIMULATION" } else { "PRODUCTION" });

    // Initialize API client
    let api = Arc::new(PolymarketApi::new(
        config.polymarket.gamma_api_url.clone(),
        config.polymarket.clob_api_url.clone(),
        config.polymarket.api_key.clone(),
    ));

    // Get market data for SOL and BTC markets
    let (sol_market_data, btc_market_data) = 
        get_or_discover_markets(&api, &config).await?;

    info!("SOL Market: {} (Condition ID: {})", sol_market_data.slug, sol_market_data.condition_id);
    info!("BTC Market: {} (Condition ID: {})", btc_market_data.slug, btc_market_data.condition_id);

    // Initialize components
    let monitor = MarketMonitor::new(
        api.clone(),
        sol_market_data,
        btc_market_data,
        config.trading.check_interval_ms,
    );
    let monitor_arc = Arc::new(monitor);

    let detector = ArbitrageDetector::new(config.trading.min_profit_threshold);
    let trader = Trader::new(
        api.clone(),
        config.trading.clone(),
        args.simulation,
    );

    // Start monitoring
    let detector_clone = detector.clone();
    let trader_arc = Arc::new(trader);
    let trader_clone = trader_arc.clone();
    let monitor_for_trading = monitor_arc.clone();
    let api_for_discovery = api.clone();
    
    // Start a background task to check pending trades periodically
    // Check every 30 seconds to catch market closures quickly (markets close after 15 minutes)
    let trader_check = trader_clone.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30)); // Check every 30 seconds
        loop {
            interval.tick().await;
            if let Err(e) = trader_check.check_pending_trades().await {
                warn!("Error checking pending trades: {}", e);
            }
        }
    });

    // Start a background task to detect new 15-minute periods and discover new markets
    let monitor_for_period_check = monitor_arc.clone();
    let api_for_period_check = api.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60)); // Check every minute
        loop {
            interval.tick().await;
            
            // Check if we need to discover new markets (new period started)
            if monitor_for_period_check.should_discover_new_markets().await {
                info!("🔄 New 15-minute period detected! Discovering new markets...");
                
                let current_time = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                
                let mut seen_ids = std::collections::HashSet::new();
                // Get current condition IDs to avoid duplicates
                let (sol_id, btc_id) = monitor_for_period_check.get_current_condition_ids().await;
                seen_ids.insert(sol_id);
                seen_ids.insert(btc_id);
                
                // Discover new markets for current period
                match discover_market(&api_for_period_check, "SOL", "sol", current_time, &mut seen_ids).await {
                    Ok(sol_market) => {
                        seen_ids.insert(sol_market.condition_id.clone());
                        match discover_market(&api_for_period_check, "BTC", "btc", current_time, &mut seen_ids).await {
                            Ok(btc_market) => {
                                if let Err(e) = monitor_for_period_check.update_markets(sol_market, btc_market).await {
                                    warn!("Failed to update markets: {}", e);
                                }
                            }
                            Err(e) => warn!("Failed to discover new BTC market: {}", e),
                        }
                    }
                    Err(e) => warn!("Failed to discover new SOL market: {}", e),
                }
            }
        }
    });
    
    monitor_arc.start_monitoring(move |snapshot| {
        let detector = detector_clone.clone();
        let trader = trader_clone.clone();
        
        async move {
            let opportunities = detector.detect_opportunities(&snapshot);
            
            for opportunity in opportunities {
                if let Err(e) = trader.execute_arbitrage(&opportunity).await {
                    warn!("Error executing trade: {}", e);
                }
            }
        }
    }).await;

    Ok(())
}

async fn get_or_discover_markets(
    api: &PolymarketApi,
    config: &Config,
) -> Result<(crate::models::Market, crate::models::Market)> {
    use crate::models::Market;
    
    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    // Try multiple discovery methods - use a set to track seen IDs
    let mut seen_ids = std::collections::HashSet::new();
    
    // Use exact slug pattern: sol-updown-15m-{timestamp} and btc-updown-15m-{timestamp}
    let sol_market = discover_market(api, "SOL", "sol", current_time, &mut seen_ids).await
        .context("Failed to discover SOL market")?;
    seen_ids.insert(sol_market.condition_id.clone());
    
    let btc_market = discover_market(api, "BTC", "btc", current_time, &mut seen_ids).await
        .context("Failed to discover BTC market")?;

    if sol_market.condition_id == btc_market.condition_id {
        anyhow::bail!("SOL and BTC markets have the same condition ID: {}. This is incorrect. Please set condition IDs manually in config.json", sol_market.condition_id);
    }

    Ok((sol_market, btc_market))
}

async fn discover_market(
    api: &PolymarketApi,
    market_name: &str,
    slug_prefix: &str,
    current_time: u64,
    seen_ids: &mut std::collections::HashSet<String>,
) -> Result<crate::models::Market> {
    use crate::models::Market;
    
    // Method 1: Try to get by slug with current timestamp (rounded to nearest 15min)
    // Pattern: btc-updown-15m-{timestamp} or sol-updown-15m-{timestamp}
    let rounded_time = (current_time / 900) * 900; // Round to nearest 15 minutes
    let slug = format!("{}-updown-15m-{}", slug_prefix, rounded_time);
    
    if let Ok(market) = api.get_market_by_slug(&slug).await {
        if !seen_ids.contains(&market.condition_id) && market.active && !market.closed {
            log::info!("Found {} market by slug: {} | Condition ID: {}", market_name, market.slug, market.condition_id);
            return Ok(market);
        }
    }
    
    // Method 2: Try a few recent timestamps in case the current one doesn't exist yet
    for offset in 1..=3 {
        let try_time = rounded_time - (offset * 900); // Try previous 15-minute intervals
        let try_slug = format!("{}-updown-15m-{}", slug_prefix, try_time);
        log::info!("Trying previous {} market by slug: {}", market_name, try_slug);
        if let Ok(market) = api.get_market_by_slug(&try_slug).await {
            if !seen_ids.contains(&market.condition_id) && market.active && !market.closed {
                log::info!("Found {} market by slug: {} | Condition ID: {}", market_name, market.slug, market.condition_id);
                return Ok(market);
            }
        }
    }
    
    anyhow::bail!("Could not find active {} 15-minute up/down market. Please set condition_id in config.json", market_name)
}
