use crate::pool::uniswap_v2::UniswapV2PoolState;
use alloy_primitives::U256;

#[derive(Debug, Clone)]
pub struct UniswapV2PoolSimulationResult {
    pub amount0_delta: U256,
    pub amount1_delta: U256,
    pub initial_state: UniswapV2PoolState,
    pub final_state: UniswapV2PoolState,
}