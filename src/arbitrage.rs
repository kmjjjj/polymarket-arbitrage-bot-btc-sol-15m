use crate::models::*;
use crate::monitor::MarketSnapshot;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

#[derive(Clone)]
pub struct ArbitrageDetector {
    min_profit_threshold: Decimal,
}

impl ArbitrageDetector {
    pub fn new(min_profit_threshold: f64) -> Self {
        Self {
            min_profit_threshold: Decimal::from_f64_retain(min_profit_threshold)
                .unwrap_or(dec!(0.01)),
        }
    }

    /// Detect arbitrage opportunities between SOL and BTC markets
    /// Strategy: Buy Up token in SOL market + Buy Down token in BTC market
    /// when total cost < $1
    pub fn detect_opportunities(&self, snapshot: &MarketSnapshot) -> Vec<ArbitrageOpportunity> {
        let mut opportunities = Vec::new();

        // Get prices from both markets
        let sol_up = snapshot.sol_market.up_token.as_ref();
        let sol_down = snapshot.sol_market.down_token.as_ref();
        let btc_up = snapshot.btc_market.up_token.as_ref();
        let btc_down = snapshot.btc_market.down_token.as_ref();

        // Strategy 1: SOL Up + BTC Down
        if let (Some(sol_up_price), Some(btc_down_price)) = (sol_up, btc_down) {
            if let Some(opportunity) = self.check_arbitrage(
                sol_up_price,
                btc_down_price,
                &snapshot.sol_market.condition_id,
                &snapshot.btc_market.condition_id,
                "SOL_UP",
                "BTC_DOWN",
            ) {
                opportunities.push(opportunity);
            }
        }

        // Strategy 2: SOL Down + BTC Up
        if let (Some(sol_down_price), Some(btc_up_price)) = (sol_down, btc_up) {
            if let Some(opportunity) = self.check_arbitrage(
                sol_down_price,
                btc_up_price,
                &snapshot.sol_market.condition_id,
                &snapshot.btc_market.condition_id,
                "SOL_DOWN",
                "BTC_UP",
            ) {
                opportunities.push(opportunity);
            }
        }

        opportunities
    }

    fn check_arbitrage(
        &self,
        token1: &TokenPrice,
        token2: &TokenPrice,
        _condition1: &str,
        _condition2: &str,
        _label1: &str,
        _label2: &str,
    ) -> Option<ArbitrageOpportunity> {
        let price1 = token1.ask_price();
        let price2 = token2.ask_price();
        let total_cost = price1 + price2;
        let dollar = dec!(1.0);
        let min_price_threshold = dec!(0.6);

        // Safety filter: Don't trade if both tokens are below $0.6 (rug case)
        // This avoids cases where both markets might go against us
        if price1 < min_price_threshold && price2 < min_price_threshold {
            return None;
        }

        // Check if total cost is less than $1
        if total_cost < dollar {
            let expected_profit = dollar - total_cost;
            
            // Only return if profit meets threshold
            if expected_profit >= self.min_profit_threshold {
                return Some(ArbitrageOpportunity {
                    sol_up_price: price1,
                    btc_down_price: price2,
                    total_cost,
                    expected_profit,
                    sol_up_token_id: token1.token_id.clone(),
                    btc_down_token_id: token2.token_id.clone(),
                    sol_condition_id: _condition1.to_string(),
                    btc_condition_id: _condition2.to_string(),
                });
            }
        }

        None
    }
}

