use crate::core::token::Token;
use crate::curve::pool_overrides::{DVariant, YVariant};
use crate::curve::strategies::{
    DefaultStrategy, DynamicFeeStrategy, LendingStrategy, MetapoolStrategy, TricryptoStrategy,
    UnscaledStrategy,
};
use alloy_primitives::{Address, U256};
use alloy_provider::Provider;
use std::sync::Arc;

/// High-level classification of a Curve pool's structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolVariant {
    Plain,
    Meta,
    Lending,
    Eth,
}

/// The specific calculation logic a pool uses, often differing in older vs newer pools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalculationStrategy {
    Legacy,
    Modern,
}

/// A comprehensive struct holding all static and semi-static configuration
/// for a Curve Stableswap pool. This separates the pool's configuration
/// from its dynamic state (like balances).
#[derive(Debug)]
pub struct PoolAttributes<> {
    pub pool_variant: PoolVariant,
    pub strategy: CalculationStrategy,
    pub swap_strategy: SwapStrategyType,
    pub d_variant: DVariant,
    pub y_variant: YVariant,
    pub n_coins: usize,
    pub rates: Vec<U256>,
    pub precision_multipliers: Vec<U256>,
    pub use_lending: Vec<bool>,
    pub fee_gamma: Option<U256>,
    pub mid_fee: Option<U256>,
    pub out_fee: Option<U256>,
    pub offpeg_fee_multiplier: Option<U256>,
    pub base_pool_address: Option<Address>,
    pub oracle_method: Option<u8>,
}

/// Holds the static attributes for a base pool, used by metapools.
/// This prevents storing dynamic state like balances within the parent pool's attributes.
// #[derive(Debug, Clone)]
// pub struct BasePoolAttributes<P: Provider + Send + Sync + 'static + ?Sized> {
//     pub address: Address,
//     pub lp_token: Arc<Token<P>>,
//     pub tokens: Vec<Arc<Token<P>>>,
//     pub n_coins: usize,
//     pub precision_multipliers: Vec<U256>,
//     pub strategy: CalculationStrategy,
//     // Base pools have their own D/Y variants for calculations like `calc_withdraw_one_coin`.
//     pub d_variant: DVariant,
//     pub y_variant: YVariant,
// }

/// An enum to represent the different swap calculation strategies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwapStrategyType {
    Default,
    Metapool,
    Lending,
    Unscaled,
    DynamicFee,
    Tricrypto,
    AdminFee,
    Oracle,
}
