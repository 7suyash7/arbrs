use crate::pool::uniswap_v2::UniswapV2PoolState;
use alloy_primitives::I256;

#[derive(Debug, Clone)]
pub struct UniswapV2PoolSimulationResult {
    pub amount0_delta: I256,
    pub amount1_delta: I256,
    pub initial_state: UniswapV2PoolState,
    pub final_state: UniswapV2PoolState,
}
