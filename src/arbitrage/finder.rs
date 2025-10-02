use crate::arbitrage::{
    cycle::TwoPoolCycle,
    types::{Arbitrage, ArbitragePath},
};
use crate::manager::{
    curve_pool_manager::CurvePoolManager, uniswap_v2_pool_manager::UniswapV2PoolManager,
    uniswap_v3_pool_manager::UniswapV3PoolManager,
};
use crate::pool::LiquidityPool;
use alloy_provider::Provider;
use itertools::Itertools;
use std::{collections::HashSet, sync::Arc};

/// Finds all 2-pool arbitrage cycles given a set of pool managers.
pub fn find_two_pool_cycles<P: Provider + Send + Sync + 'static + ?Sized>(
    v2_manager: &UniswapV2PoolManager<P>,
    v3_manager: &UniswapV3PoolManager<P>,
    curve_manager: &CurvePoolManager<P>,
) -> Vec<Arc<dyn Arbitrage<P>>> {
    let mut all_pools: Vec<Arc<dyn LiquidityPool<P>>> = Vec::new();

    all_pools.extend(v2_manager.get_all_pools());
    all_pools.extend(v3_manager.get_all_pools());
    all_pools.extend(curve_manager.get_all_pools());

    println!("Finding 2-pool cycles across {} total pools...", all_pools.len());

    let mut arbitrage_paths: Vec<Arc<dyn Arbitrage<P>>> = Vec::new();

    for pool_pair in all_pools.into_iter().combinations(2) {
        let pool_a = &pool_pair[0];
        let pool_b = &pool_pair[1];

        let tokens_a: HashSet<_> = pool_a.get_all_tokens().into_iter().collect();
        let tokens_b: HashSet<_> = pool_b.get_all_tokens().into_iter().collect();
        let common_tokens: Vec<_> = tokens_a.intersection(&tokens_b).cloned().collect();

        if common_tokens.len() >= 2 {
            for token_pair in common_tokens.into_iter().combinations(2) {
                let token0 = token_pair[0].clone();
                let token1 = token_pair[1].clone();

                // Path 1: A -> B -> A via Pool A then Pool B
                let path1 = ArbitragePath {
                    pools: vec![pool_a.clone(), pool_b.clone()],
                    path: vec![token0.clone(), token1.clone(), token0.clone()],
                    profit_token: token0.clone(),
                };
                arbitrage_paths.push(Arc::new(TwoPoolCycle::new(path1)));

                // Path 2: B -> A -> B via Pool A then Pool B
                let path2 = ArbitragePath {
                    pools: vec![pool_a.clone(), pool_b.clone()],
                    path: vec![token1.clone(), token0.clone(), token1.clone()],
                    profit_token: token1.clone(),
                };
                arbitrage_paths.push(Arc::new(TwoPoolCycle::new(path2)));
            }
        }
    }

    arbitrage_paths
}