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
use alloy_primitives::{address, Address};
use alloy_provider::Provider;
use itertools::Itertools;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};

#[derive(Debug, Clone)]
struct PathInSearch<P: Provider + Send + Sync + 'static + ?Sized> {
    pub pools: Vec<Arc<dyn LiquidityPool<P>>>,
    pub tokens: Vec<Arc<Token<P>>>,
    pub current_token: Arc<Token<P>>, 
}

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

fn get_canonical_cycle_path<P>(pools: &[Arc<dyn LiquidityPool<P>>]) -> Vec<Address>
where
    P: Provider + Send + Sync + 'static + ?Sized,
{
    let addresses: Vec<Address> = pools.iter().map(|p| p.address()).collect();
    let n = addresses.len();

    if n == 0 {
        return Vec::new();
    }

    let mut canonical = addresses.clone();

    let mut min_index = 0;
    for i in 1..n {
        if addresses[i] < addresses[min_index] {
            min_index = i;
        }
    }

    let mut normalized = Vec::with_capacity(n);
    for i in 0..n {
        normalized.push(addresses[(min_index + i) % n]);
    }

    let mut reversed = normalized.clone();
    reversed.reverse();

    if reversed < normalized {
        canonical = reversed;
    } else {
        canonical = normalized;
    }

    canonical
}

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
    find_multi_hop_cycles(
        v2_manager,
        v3_manager,
        curve_manager,
        balancer_manager,
        token_manager,
        3,
    )
    .await
}

pub async fn find_multi_hop_cycles<P>(
    v2_manager: &UniswapV2PoolManager<P>,
    v3_manager: &UniswapV3PoolManager<P>,
    curve_manager: &CurvePoolManager<P>,
    balancer_manager: &BalancerPoolManager<P>,
    token_manager: &TokenManager<P>,
    max_hops: usize,
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

    let mut canonical_cycles: HashSet<Vec<Address>> = HashSet::new(); 

    let start_token = match token_manager
        .get_token(address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"))
        .await
    {
        Ok(token) => token,
        Err(_) => return Vec::new(),
    };

    let mut queue: VecDeque<PathInSearch<P>> = VecDeque::new();

    if let Some(neighbors) = graph.get(&start_token) {
        for neighbor in neighbors {
            let path = PathInSearch {
                pools: vec![neighbor.pool.clone()],
                tokens: vec![start_token.clone(), neighbor.token.clone()],
                current_token: neighbor.token.clone(),
            };
            queue.push_back(path);
        }
    }

    while let Some(current_path) = queue.pop_front() {
        let current_hop = current_path.pools.len();

        if current_hop >= max_hops { 
            continue;
        }

        if let Some(neighbors) = graph.get(&current_path.current_token) {
            for neighbor in neighbors {
                let next_token = &neighbor.token;
                let next_pool = &neighbor.pool;

                if next_token.address() == start_token.address() {
                    let new_pools = [current_path.pools.clone(), vec![next_pool.clone()]].concat();
                    let new_tokens = [current_path.tokens.clone(), vec![start_token.clone()]].concat();

                    if new_pools.len() >= 2 {
                        let canonical = get_canonical_cycle_path(&new_pools);
                        
                        if !canonical_cycles.contains(&canonical) {
                            canonical_cycles.insert(canonical);

                            let arbitrage_path = ArbitragePath {
                                pools: new_pools,
                                path: new_tokens,
                                profit_token: start_token.clone(),
                            };
                            
                            arbitrage_paths.push(Arc::new(ArbitrageCycle::new(arbitrage_path)));
                        }
                    }
                }
                else {
                    let previous_token = &current_path.tokens[current_path.tokens.len() - 2];
                    if next_token.address() != previous_token.address() {
                        let next_path = PathInSearch {
                            pools: [current_path.pools.clone(), vec![next_pool.clone()]].concat(),
                            tokens: [current_path.tokens.clone(), vec![next_token.clone()]].concat(),
                            current_token: next_token.clone(),
                        };
                        queue.push_back(next_path);
                    }
                }
            }
        }
    }
    
    tracing::info!(
        "Found {} unique multi-hop arbitrage paths (up to {} hops).",
        arbitrage_paths.len(), max_hops
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
