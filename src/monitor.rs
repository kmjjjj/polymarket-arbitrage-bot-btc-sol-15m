use crate::api::PolymarketApi;
use crate::models::*;
use anyhow::Result;
use log::{debug, info, warn};
use std::sync::Arc;
use tokio::time::{sleep, Duration};

pub struct MarketMonitor {
    api: Arc<PolymarketApi>,
    sol_market: Arc<tokio::sync::Mutex<crate::models::Market>>,
    btc_market: Arc<tokio::sync::Mutex<crate::models::Market>>,
    check_interval: Duration,
    // Cached token IDs from getMarket() - refreshed once per period
    sol_up_token_id: Arc<tokio::sync::Mutex<Option<String>>>,
    sol_down_token_id: Arc<tokio::sync::Mutex<Option<String>>>,
    btc_up_token_id: Arc<tokio::sync::Mutex<Option<String>>>,
    btc_down_token_id: Arc<tokio::sync::Mutex<Option<String>>>,
    last_market_refresh: Arc<tokio::sync::Mutex<Option<std::time::Instant>>>,
    current_period_timestamp: Arc<tokio::sync::Mutex<u64>>, // Track current 15-minute period
}

#[derive(Debug, Clone)]
pub struct MarketSnapshot {
    pub sol_market: MarketData,
    pub btc_market: MarketData,
    pub timestamp: std::time::Instant,
}

impl MarketMonitor {
    pub fn new(
        api: Arc<PolymarketApi>,
        sol_market: crate::models::Market,
        btc_market: crate::models::Market,
        check_interval_ms: u64,
    ) -> Self {
        // Calculate current 15-minute period timestamp
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let current_period = (current_time / 900) * 900; // Round to nearest 15 minutes
        
        Self {
            api,
            sol_market: Arc::new(tokio::sync::Mutex::new(sol_market)),
            btc_market: Arc::new(tokio::sync::Mutex::new(btc_market)),
            check_interval: Duration::from_millis(check_interval_ms),
            sol_up_token_id: Arc::new(tokio::sync::Mutex::new(None)),
            sol_down_token_id: Arc::new(tokio::sync::Mutex::new(None)),
            btc_up_token_id: Arc::new(tokio::sync::Mutex::new(None)),
            btc_down_token_id: Arc::new(tokio::sync::Mutex::new(None)),
            last_market_refresh: Arc::new(tokio::sync::Mutex::new(None)),
            current_period_timestamp: Arc::new(tokio::sync::Mutex::new(current_period)),
        }
    }

    /// Update markets when a new 15-minute period starts
    pub async fn update_markets(&self, sol_market: crate::models::Market, btc_market: crate::models::Market) -> Result<()> {
        info!("🔄 Updating to new 15-minute period markets...");
        info!("New SOL Market: {} ({})", sol_market.slug, sol_market.condition_id);
        info!("New BTC Market: {} ({})", btc_market.slug, btc_market.condition_id);
        
        *self.sol_market.lock().await = sol_market;
        *self.btc_market.lock().await = btc_market;
        
        // Reset token IDs - will be refreshed on next fetch
        *self.sol_up_token_id.lock().await = None;
        *self.sol_down_token_id.lock().await = None;
        *self.btc_up_token_id.lock().await = None;
        *self.btc_down_token_id.lock().await = None;
        *self.last_market_refresh.lock().await = None;
        
        // Update current period timestamp
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let new_period = (current_time / 900) * 900;
        *self.current_period_timestamp.lock().await = new_period;
        
        Ok(())
    }

    /// Check if we need to discover new markets (new 15-minute period started)
    pub async fn should_discover_new_markets(&self) -> bool {
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let current_period = (current_time / 900) * 900;
        
        let stored_period = *self.current_period_timestamp.lock().await;
        
        // If current period is different from stored period, we need new markets
        current_period != stored_period
    }

    /// Get current market condition IDs (for checking if markets are closed)
    pub async fn get_current_condition_ids(&self) -> (String, String) {
        let sol = self.sol_market.lock().await.condition_id.clone();
        let btc = self.btc_market.lock().await.condition_id.clone();
        (sol, btc)
    }

    /// Refresh market data once per period (15 minutes) to get token IDs
    async fn refresh_market_tokens(&self) -> Result<()> {
        // Check if we need to refresh (every 15 minutes = 900 seconds)
        let should_refresh = {
            let last_refresh = self.last_market_refresh.lock().await;
            last_refresh
                .map(|last| last.elapsed().as_secs() >= 900)
                .unwrap_or(true)
        };

        if !should_refresh {
            return Ok(());
        }


        let (sol_condition_id, btc_condition_id) = self.get_current_condition_ids().await;

        // Get SOL market details
        if let Ok(sol_details) = self.api.get_market(&sol_condition_id).await {
            for token in &sol_details.tokens {
                let outcome_upper = token.outcome.to_uppercase();
                if outcome_upper.contains("UP") || outcome_upper == "1" {
                    *self.sol_up_token_id.lock().await = Some(token.token_id.clone());
                    info!("SOL Up token_id: {}", token.token_id);
                } else if outcome_upper.contains("DOWN") || outcome_upper == "0" {
                    *self.sol_down_token_id.lock().await = Some(token.token_id.clone());
                    info!("SOL Down token_id: {}", token.token_id);
                }
            }
        }

        // Get BTC market details
        if let Ok(btc_details) = self.api.get_market(&btc_condition_id).await {
            for token in &btc_details.tokens {
                let outcome_upper = token.outcome.to_uppercase();
                if outcome_upper.contains("UP") || outcome_upper == "1" {
                    *self.btc_up_token_id.lock().await = Some(token.token_id.clone());
                    info!("BTC Up token_id: {}", token.token_id);
                } else if outcome_upper.contains("DOWN") || outcome_upper == "0" {
                    *self.btc_down_token_id.lock().await = Some(token.token_id.clone());
                    info!("BTC Down token_id: {}", token.token_id);
                }
            }
        }

        *self.last_market_refresh.lock().await = Some(std::time::Instant::now());
        Ok(())
    }

    /// Fetch current market data for both SOL and BTC markets
    /// Uses get_price() endpoint continuously for real-time prices
    pub async fn fetch_market_data(&self) -> Result<MarketSnapshot> {
        // Refresh token IDs if needed (once per 15-minute period)
        self.refresh_market_tokens().await?;

        let (sol_condition_id, btc_condition_id) = self.get_current_condition_ids().await;
        
        // Fetch prices for all tokens using the price endpoint
        let sol_up_token_id = self.sol_up_token_id.lock().await.clone();
        let sol_down_token_id = self.sol_down_token_id.lock().await.clone();
        let btc_up_token_id = self.btc_up_token_id.lock().await.clone();
        let btc_down_token_id = self.btc_down_token_id.lock().await.clone();
        
        let (sol_up_price, sol_down_price, btc_up_price, btc_down_price) = tokio::join!(
            self.fetch_token_price(&sol_up_token_id, "SOL", "Up"),
            self.fetch_token_price(&sol_down_token_id, "SOL", "Down"),
            self.fetch_token_price(&btc_up_token_id, "BTC", "Up"),
            self.fetch_token_price(&btc_down_token_id, "BTC", "Down"),
        );

        let sol_market_data = MarketData {
            condition_id: sol_condition_id,
            market_name: "SOL".to_string(),
            up_token: sol_up_price,
            down_token: sol_down_price,
        };

        let btc_market_data = MarketData {
            condition_id: btc_condition_id,
            market_name: "BTC".to_string(),
            up_token: btc_up_price,
            down_token: btc_down_price,
        };

        Ok(MarketSnapshot {
            sol_market: sol_market_data,
            btc_market: btc_market_data,
            timestamp: std::time::Instant::now(),
        })
    }

    async fn fetch_token_price(
        &self,
        token_id: &Option<String>,
        market_name: &str,
        outcome: &str,
    ) -> Option<TokenPrice> {
        let token_id = token_id.as_ref()?;

        // Get BUY price (ask price - what we pay to buy)
        let buy_price = match self.api.get_price(token_id, "BUY").await {
            Ok(price) => Some(price),
            Err(e) => {
                warn!("Failed to fetch {} {} BUY price: {}", market_name, outcome, e);
                None
            }
        };

        // Get SELL price (bid price - what we get when selling)
        let sell_price = match self.api.get_price(token_id, "SELL").await {
            Ok(price) => Some(price),
            Err(e) => {
                warn!("Failed to fetch {} {} SELL price: {}", market_name, outcome, e);
                None
            }
        };

        if buy_price.is_some() || sell_price.is_some() {
            Some(TokenPrice {
                token_id: token_id.clone(),
                bid: sell_price,
                ask: buy_price,
            })
        } else {
            None
        }
    }


    /// Start monitoring markets continuously
    /// Returns a callback function that can be used to update markets when new period starts
    pub async fn start_monitoring<F, Fut>(&self, callback: F)
    where
        F: Fn(MarketSnapshot) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        info!("Starting market monitoring...");
        
        loop {
            match self.fetch_market_data().await {
                Ok(snapshot) => {
                    debug!("Market snapshot updated");
                    callback(snapshot).await;
                }
                Err(e) => {
                    warn!("Error fetching market data: {}", e);
                }
            }
            
            sleep(self.check_interval).await;
        }
    }
}

