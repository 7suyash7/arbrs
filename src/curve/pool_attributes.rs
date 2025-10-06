use crate::curve::pool_overrides::{DVariant, YVariant};
use alloy_primitives::{Address, U256};
use serde::{Deserialize, Serialize};

/// High-level classification of a Curve pool's structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PoolVariant {
    Plain,
    Meta,
    Lending,
    Eth,
}

/// The specific calculation logic a pool uses, often differing in older vs newer pools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CalculationStrategy {
    Legacy,
    Modern,
}

/// A comprehensive struct holding all static and semi-static configuration
/// for a Curve Stableswap pool. This separates the pool's configuration
/// from its dynamic state (like balances).
#[derive(Debug, Serialize, Deserialize)]
pub struct PoolAttributes {
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

/// An enum to represent the different swap calculation strategies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
