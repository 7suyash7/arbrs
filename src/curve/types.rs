use alloy_primitives::{Address, I256, U256};

/// Holds the state of a Curve Stableswap pool at a specific block.
#[derive(Clone, Debug, Default)]
pub struct CurveStableswapPoolState {
    pub address: Address,
    pub block_number: u64,
    pub balances: Vec<U256>,
    /// For metapools, this holds the state of the base pool.
    pub base_pool_state: Option<Box<CurveStableswapPoolState>>,
}

/// Represents the result of a simulated swap on a Curve pool.
#[derive(Debug, Clone)]
pub struct CurveStableswapPoolSimulationResult {
    pub amount0_delta: I256,
    pub amount1_delta: I256,
    pub initial_state: CurveStableswapPoolState,
    pub final_state: CurveStableswapPoolState,
}

/// Holds the static attributes of a Curve Stableswap pool.
#[derive(Debug, Clone)]
pub struct CurveStableSwapPoolAttributes {
    pub address: Address,
    pub lp_token_address: Address,
    pub coin_addresses: Vec<Address>,
    pub coin_index_type: String,
    pub is_metapool: bool,
    pub underlying_coin_addresses: Option<Vec<Address>>,
    pub base_pool_address: Option<Address>,
}

/// A message indicating that a Curve pool's state has been updated.
#[derive(Debug, Clone)]
pub struct CurveStableSwapPoolStateUpdated {
    pub state: CurveStableswapPoolState,
}

#[derive(Clone, Debug, Default)]
pub struct CurvePoolSnapshot {
    pub balances: Vec<U256>,
    pub a: U256,
    pub fee: U256,
    pub block_timestamp: u64,
    pub base_pool_virtual_price: Option<U256>,
    pub base_pool_lp_total_supply: Option<U256>,

    // Data for complex strategies
    pub rates: Vec<U256>,
    pub admin_balances: Option<Vec<U256>>,

    // Tricrypto-specific data
    pub tricrypto_d: Option<U256>,
    pub tricrypto_gamma: Option<U256>,
    pub tricrypto_price_scale: Option<Vec<U256>>,

    // Metapool-specific data
    pub scaled_redemption_price: Option<U256>,
}
