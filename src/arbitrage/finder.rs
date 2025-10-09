use crate::{
    TokenLike, TokenManager,
    arbitrage::{
        cycle::ArbitrageCycle,
        types::{Arbitrage, ArbitragePath},
    },
    core::token::Token,
    manager::{
        balancer_pool_manager::BalancerPoolManager, curve_pool_manager::CurvePoolManager,
        uniswap_v2_pool_manager::UniswapV2PoolManager,
        uniswap_v3_pool_manager::UniswapV3PoolManager,
    },
    pool::LiquidityPool,
};
use alloy_primitives::address;
use alloy_provider::Provider;
use itertools::Itertools;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[derive(Clone, Debug)]
pub struct PoolNeighbor<P: Provider + Send + Sync + 'static + ?Sized> {
    pub pool: Arc<dyn LiquidityPool<P>>,
    pub token: Arc<Token<P>>,
}
type AdjacencyList<P> = HashMap<Arc<Token<P>>, Vec<PoolNeighbor<P>>>;
fn build_graph<P>(all_pools: Vec<Arc<dyn LiquidityPool<P>>>) -> AdjacencyList<P>
where
    P: Provider + Send + Sync + 'static + ?Sized,
{
    let mut graph: AdjacencyList<P> = HashMap::new();
    tracing::info!("Building market graph from {} pools...", all_pools.len());

    for pool in all_pools {
        let tokens = pool.get_all_tokens();
        for token_pair in tokens.into_iter().combinations(2) {
            let token0 = token_pair[0].clone();
            let token1 = token_pair[1].clone();

            graph.entry(token0.clone()).or_default().push(PoolNeighbor {
                pool: pool.clone(),
                token: token1.clone(),
            });

            graph.entry(token1).or_default().push(PoolNeighbor {
                pool: pool.clone(),
                token: token0,
            });
        }
    }

    tracing::info!("Graph built with {} unique tokens (nodes).", graph.len());
    graph
}

// 3-POOL CYCLE FINDER

pub async fn find_three_pool_cycles<P>(
    v2_manager: &UniswapV2PoolManager<P>,
    v3_manager: &UniswapV3PoolManager<P>,
    curve_manager: &CurvePoolManager<P>,
    balancer_manager: &BalancerPoolManager<P>,
    token_manager: &TokenManager<P>,
) -> Vec<Arc<dyn Arbitrage<P>>>
where
    P: Provider + Send + Sync + 'static + ?Sized,
{
    let mut all_pools: Vec<Arc<dyn LiquidityPool<P>>> = Vec::new();
    all_pools.extend(v2_manager.get_all_pools());
    all_pools.extend(v3_manager.get_all_pools());
    all_pools.extend(curve_manager.get_all_pools());
    all_pools.extend(balancer_manager.get_all_pools());

    if all_pools.is_empty() {
        return Vec::new();
    }

    let graph = build_graph(all_pools);
    let mut arbitrage_paths: Vec<Arc<dyn Arbitrage<P>>> = Vec::new();

    let start_token = match token_manager
        .get_token(address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"))
        .await
    {
        Ok(token) => token,
        Err(_) => return Vec::new(), // Cannot proceed without the start token
    };

    tracing::info!(
        "Starting 3-pool cycle search from token: {}",
        start_token.symbol()
    );

    // Find all direct neighbors of the start_token. (Path: A -> B)
    if let Some(neighbors1) = graph.get(&start_token) {
        for neighbor1 in neighbors1 {
            let pool1 = &neighbor1.pool;
            let token1 = &neighbor1.token;

            // From each neighbor, find *their* neighbors. (Path: A -> B -> C)
            if let Some(neighbors2) = graph.get(token1) {
                for neighbor2 in neighbors2 {
                    let pool2 = &neighbor2.pool;
                    let token2 = &neighbor2.token;

                    // Avoid going immediately back (A -> B -> A)
                    if token2.address() == start_token.address() {
                        continue;
                    }

                    // LEVEL 3: From the new token, check if it can swap back to the start. (Path: A -> B -> C -> A)
                    if let Some(neighbors3) = graph.get(token2) {
                        for neighbor3 in neighbors3 {
                            if neighbor3.token.address() == start_token.address() {
                                let pool3 = &neighbor3.pool;

                                // CYCLE DETECTED!
                                let path = ArbitragePath {
                                    pools: vec![pool1.clone(), pool2.clone(), pool3.clone()],
                                    path: vec![
                                        start_token.clone(),
                                        token1.clone(),
                                        token2.clone(),
                                        start_token.clone(),
                                    ],
                                    profit_token: start_token.clone(),
                                };
                                arbitrage_paths.push(Arc::new(ArbitrageCycle::new(path)));
                            }
                        }
                    }
                }
            }
        }
    }

    // Deduplicate paths that might be found in reverse
    arbitrage_paths.dedup_by(|a, b| {
        let path_a = a
            .as_any()
            .downcast_ref::<ArbitrageCycle<P>>()
            .unwrap()
            .path
            .clone();
        let path_b = b
            .as_any()
            .downcast_ref::<ArbitrageCycle<P>>()
            .unwrap()
            .path
            .clone();

        let pools_a: Vec<_> = path_a.pools.iter().map(|p| p.address()).collect();
        let mut pools_b: Vec<_> = path_b.pools.iter().map(|p| p.address()).collect();
        pools_b.reverse();

        pools_a == pools_b
    });

    tracing::info!(
        "Found {} potential 3-pool arbitrage paths.",
        arbitrage_paths.len()
    );
    arbitrage_paths
}

/// Finds all 2-pool arbitrage cycles given a set of pool managers.
pub fn find_two_pool_cycles<P: Provider + Send + Sync + 'static + ?Sized>(
    v2_manager: &UniswapV2PoolManager<P>,
    v3_manager: &UniswapV3PoolManager<P>,
    curve_manager: &CurvePoolManager<P>,
    balancer_manager: &BalancerPoolManager<P>,
) -> Vec<Arc<dyn Arbitrage<P>>> {
    let mut all_pools: Vec<Arc<dyn LiquidityPool<P>>> = Vec::new();

    all_pools.extend(v2_manager.get_all_pools());
    all_pools.extend(v3_manager.get_all_pools());
    all_pools.extend(curve_manager.get_all_pools());
    all_pools.extend(balancer_manager.get_all_pools());

    tracing::info!(
        "Finding 2-pool cycles across {} total pools...",
        all_pools.len()
    );
    println!(
        "Finding 2-pool cycles across {} total pools...",
        all_pools.len()
    );

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
                arbitrage_paths.push(Arc::new(ArbitrageCycle::new(path1)));

                // Path 2: B -> A -> B via Pool A then Pool B
                let path2 = ArbitragePath {
                    pools: vec![pool_a.clone(), pool_b.clone()],
                    path: vec![token1.clone(), token0.clone(), token1.clone()],
                    profit_token: token1.clone(),
                };
                arbitrage_paths.push(Arc::new(ArbitrageCycle::new(path2)));
            }
        }
    }
    arbitrage_paths
}
