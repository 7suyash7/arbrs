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

#[async_trait]
pub trait LiquidityPool<P: Provider + Send + Sync + 'static + ?Sized>: Debug + Send + Sync {
    /// Returns the pool's contract address.
    fn address(&self) -> Address;

    /// Returns the pair of tokens in the pool as (token0, token1).
    fn tokens(&self) -> (Arc<Token<P>>, Arc<Token<P>>);

    /// Fetches the latest state (e.g., reserves) from the blockchain and updates the internal state.
    async fn update_state(&self) -> Result<(), ArbRsError>;

    /// Calculates the output amount for a given input token and amount based on current reserves.
    async fn calculate_tokens_out(
        &self,
        token_in: &Token<P>,
        amount_in: U256,
    ) -> Result<U256, ArbRsError>;

    /// Calculates the required input amount to receive a specific output amount.
    async fn calculate_tokens_in_from_tokens_out(
        &self,
        token_out: &Token<P>,
        amount_out: U256,
    ) -> Result<U256, ArbRsError>;

    /// Calculates the "nominal price" of token0 in terms of token1, scaled by decimals.
    /// Price a_to_b = reserve_b / reserve_a * 10^(decimals_a - decimals_b)
    async fn nominal_price(&self) -> Result<f64, ArbRsError>;

    /// Calculates the "absolute price" of token0 in terms of token1, without decimal scaling.
    /// Price a_to_b = reserve_b / reserve_a
    async fn absolute_price(&self) -> Result<f64, ArbRsError>;

    /// Downcasting methjod
    fn as_any(&self) -> &dyn Any;
}
