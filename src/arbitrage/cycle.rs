use crate::arbitrage::types::{Arbitrage, ArbitragePath};
use crate::errors::ArbRsError;
use alloy_primitives::U256;
use alloy_provider::Provider;
use async_trait::async_trait;
use std::fmt::{self, Debug};
use std::sync::Arc;

/// Represents a simple two-pool arbitrage cycle (e.g., WETH -> USDC -> WETH).
#[derive(Clone)]
pub struct TwoPoolCycle<P: Provider + Send + Sync + 'static + ?Sized> {
    pub path: Arc<ArbitragePath<P>>,
}

impl<P: Provider + Send + Sync + 'static + ?Sized> TwoPoolCycle<P> {
    pub fn new(path: ArbitragePath<P>) -> Self {
        Self {
            path: Arc::new(path),
        }
    }
}

#[async_trait]
impl<P: Provider + Send + Sync + 'static + ?Sized> Arbitrage<P> for TwoPoolCycle<P> {
    async fn calculate_profit(
        &self,
        start_amount: U256,
        block_number: Option<u64>,
    ) -> Result<(U256, U256), ArbRsError> {
        let amount_out_b = self
            .path
            .pools[0]
            .calculate_tokens_out(&self.path.path[0], &self.path.path[1], start_amount, block_number)
            .await?;

        if amount_out_b.is_zero() {
            return Ok((U256::ZERO, U256::ZERO));
        }

        let final_amount_out = self
            .path
            .pools[1]
            .calculate_tokens_out(&self.path.path[1], &self.path.path[2], amount_out_b, block_number)
            .await?;

        let profit = final_amount_out.saturating_sub(start_amount);

        Ok((profit, final_amount_out))
    }
}

impl<P: Provider + Send + Sync + 'static + ?Sized> Debug for TwoPoolCycle<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwoPoolCycle")
            .field("path", &self.path)
            .finish()
    }
}
