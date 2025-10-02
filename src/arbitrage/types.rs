use crate::core::token::Token;
use crate::errors::ArbRsError;
use crate::pool::LiquidityPool;
use alloy_primitives::U256;
use alloy_provider::Provider;
use async_trait::async_trait;
use std::fmt::{self, Debug};
use std::sync::Arc;

/// Represents a potential arbitrage opportunity, defining the sequence of pools
/// and tokens to be traded.
#[derive(Clone)]
pub struct ArbitragePath<P: Provider + Send + Sync + 'static + ?Sized> {
    /// The sequence of liquidity pools to trade through.
    pub pools: Vec<Arc<dyn LiquidityPool<P>>>,
    /// The sequence of tokens to trade. The first and last token should be the same.
    pub path: Vec<Arc<Token<P>>>,
    /// The token that will be used to measure profit.
    pub profit_token: Arc<Token<P>>,
}

/// A trait representing a generic arbitrage strategy.
#[async_trait]
pub trait Arbitrage<P: Provider + Send + Sync + 'static + ?Sized>: Debug + Send + Sync {
    /// Calculates the potential profit for a given starting amount.
    ///
    /// Returns a tuple of `(profit, amount_out)`, where `profit` is the net gain
    /// and `amount_out` is the total amount of the profit token returned at the end.
    async fn calculate_profit(
        &self,
        start_amount: U256,
        block_number: Option<u64>,
    ) -> Result<(U256, U256), ArbRsError>;
}

impl<P: Provider + Send + Sync + 'static + ?Sized> Debug for ArbitragePath<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ArbitragePath")
            .field(
                "pools",
                &self.pools.iter().map(|p| p.address()).collect::<Vec<_>>(),
            )
            .field("path", &self.path)
            .field("profit_token", &self.profit_token)
            .finish()
    }
}
