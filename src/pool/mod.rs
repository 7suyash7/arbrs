use crate::core::token::Token;
use crate::errors::ArbRsError;
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

#[async_trait]
pub trait LiquidityPool<P: Provider + Send + Sync + 'static + ?Sized>: Debug + Send + Sync {
    /// Returns the pool's contract address.
    fn address(&self) -> Address;

    /// Returns a vector of all tokens in the pool.
    fn get_all_tokens(&self) -> Vec<Arc<Token<P>>>;

    /// Fetches the latest state from the blockchain.
    async fn update_state(&self) -> Result<(), ArbRsError>;

    /// Calculates the output amount for a given input and output token.
    async fn calculate_tokens_out(
        &self,
        token_in: &Token<P>,
        token_out: &Token<P>,
        amount_in: U256,
        block_number: Option<u64>,
    ) -> Result<U256, ArbRsError>;

    /// Calculates the required input amount for a given input and output token.
    async fn calculate_tokens_in_from_tokens_out(
        &self,
        token_in: &Token<P>,
        token_out: &Token<P>,
        amount_out: U256,
    ) -> Result<U256, ArbRsError>;

    /// Calculates the "absolute exchange rate" of token1 in terms of token0, without decimal scaling.
    /// Rate b_to_a = reserve_a / reserve_b
    async fn nominal_price(
        &self,
        token_in: &Token<P>,
        token_out: &Token<P>,
    ) -> Result<f64, ArbRsError>;

    /// Calculates the "absolute price" of token0 in terms of token1, without decimal scaling.
    /// Price a_to_b = reserve_b / reserve_a
    async fn absolute_price(
        &self,
        token_in: &Token<P>,
        token_out: &Token<P>,
    ) -> Result<f64, ArbRsError>;

    /// Calculates the "absolute exchange rate" of token1 in terms of token0, without decimal scaling.
    /// Rate b_to_a = reserve_a / reserve_b
    async fn absolute_exchange_rate(
        &self,
        token_in: &Token<P>,
        token_out: &Token<P>,
    ) -> Result<f64, ArbRsError>;

    fn as_any(&self) -> &dyn Any;
}
