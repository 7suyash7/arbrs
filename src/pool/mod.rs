use crate::core::token::Token;
use crate::curve::types::CurvePoolSnapshot;
use crate::errors::ArbRsError;
use crate::pool::uniswap_v2::UniswapV2PoolState;
use crate::pool::uniswap_v3::UniswapV3PoolSnapshot;
use crate::balancer::pool::BalancerPoolSnapshot;
use alloy_primitives::{Address, U256};
use alloy_provider::Provider;
use async_trait::async_trait;
use std::any::Any;
use std::fmt::Debug;
use std::sync::Arc;

pub mod strategy;
pub mod uniswap_v2;
pub mod uniswap_v2_simulation;
pub mod uniswap_v3;
pub mod uniswap_v3_snapshot;

#[derive(Debug, Clone)]
pub struct UniswapPoolSwapVector<P: Provider + Send + Sync + 'static + ?Sized> {
    pub token_in: Arc<Token<P>>,
    pub token_out: Arc<Token<P>>,
    pub zero_for_one: bool,
}

#[derive(Debug, Clone)]
pub enum PoolSnapshot {
    UniswapV2(UniswapV2PoolState),
    UniswapV3(UniswapV3PoolSnapshot),
    Curve(CurvePoolSnapshot),
    Balancer(BalancerPoolSnapshot),
}

#[async_trait]
pub trait LiquidityPool<P: Provider + Send + Sync + 'static + ?Sized>: Debug + Send + Sync {
    /// Returns the pool's contract address.
    fn address(&self) -> Address;

    /// Returns a vector of all tokens in the pool.
    fn get_all_tokens(&self) -> Vec<Arc<Token<P>>>;

    /// Fetches the latest state from the blockchain and updates the pool's internal cache.
    async fn update_state(&self) -> Result<(), ArbRsError>;

    /// Fetches all dynamic data for a pool at a specific block and returns a snapshot.
    async fn get_snapshot(&self, block_number: Option<u64>) -> Result<PoolSnapshot, ArbRsError>;

    /// Calculates tokens out using a pre-fetched state snapshot. PURE & SYNCHRONOUS.
    fn calculate_tokens_out(
        &self,
        token_in: &Token<P>,
        token_out: &Token<P>,
        amount_in: U256,
        snapshot: &PoolSnapshot,
    ) -> Result<U256, ArbRsError>;

    /// Calculates tokens in from a pre-fetched state snapshot. PURE & SYNCHRONOUS.
    fn calculate_tokens_in(
        &self,
        token_in: &Token<P>,
        token_out: &Token<P>,
        amount_out: U256,
        snapshot: &PoolSnapshot,
    ) -> Result<U256, ArbRsError>;

    /// Calculates the "absolute price" of token0 in terms of token1, without decimal scaling.
    async fn absolute_price(
        &self,
        token_in: &Token<P>,
        token_out: &Token<P>,
    ) -> Result<f64, ArbRsError>;
    
    async fn nominal_price(
        &self,
        token_in: &Token<P>,
        token_out: &Token<P>,
    ) -> Result<f64, ArbRsError>;
        
    async fn absolute_exchange_rate(
        &self,
        token_in: &Token<P>,
        token_out: &Token<P>,
    ) -> Result<f64, ArbRsError>;

    fn as_any(&self) -> &dyn Any;
}
