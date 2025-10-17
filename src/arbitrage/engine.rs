use crate::{arbitrage::{
    cache::ArbitrageCache, cycle::ArbitrageCycle, optimizer, types::{Arbitrage, ArbitrageSolution, SwapAction},
}, pool::{LiquidityPool, PoolSnapshot}, ArbRsError, Token, TokenLike, TokenManager};
use alloy_primitives::{address, Address, U256};
use alloy_provider::Provider;
use futures::{future::join_all, StreamExt};
use std::{
    collections::{HashMap, HashSet},
    fmt::{self, Debug},
    sync::Arc,
};

const WETH_ADDRESS: Address = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");

/// The main engine responsible for evaluating arbitrage opportunities.
pub struct ArbitrageEngine<P: Provider + Send + Sync + 'static + ?Sized> {
    pub cache: Arc<ArbitrageCache<P>>,
    pub token_manager: Arc<TokenManager<P>>,
    pub provider: Arc<P>,
}

impl<P: Provider + Send + Sync + 'static + ?Sized> ArbitrageEngine<P> {
    pub fn new(
        cache: Arc<ArbitrageCache<P>>,
        token_manager: Arc<TokenManager<P>>,
        provider: Arc<P>,
    ) -> Self {
        Self { cache, token_manager, provider }
    }

    async fn get_all_profit_token_conversion_rates(
        &self,
        paths: &Vec<Arc<dyn Arbitrage<P>>>,
        all_pools: &HashMap<Address, Arc<dyn LiquidityPool<P>>>,
    ) -> HashMap<Address, U256> {
        let token_manager = self.token_manager.clone(); 

        let weth_token = match token_manager.get_token(WETH_ADDRESS).await {
            Ok(t) => t,
            Err(_) => return HashMap::new(),
        };

        let unique_profit_tokens: HashSet<Arc<Token<P>>> = paths.iter()
            .filter_map(|path| path.as_any().downcast_ref::<ArbitrageCycle<P>>())
            .map(|cycle| cycle.path.profit_token.clone())
            .collect();
        
        let mut rate_map: HashMap<Address, U256> = HashMap::new();

        let rate_futs = unique_profit_tokens.into_iter().map(|profit_token| {
            let pools_ref = all_pools.clone();
            let weth_token_clone = weth_token.clone();
            
            async move {
                if profit_token.address() == WETH_ADDRESS {
                    return (profit_token.address(), Ok(U256::from_limbs([1_000_000_000_000_000_000, 0, 0, 0])));
                }

                if let Some((_, pool)) = pools_ref.iter().find(|(_, p)| {
                    let tokens: Vec<Address> = p.get_all_tokens().iter().map(|t| t.address()).collect();
                    tokens.contains(&WETH_ADDRESS) && tokens.contains(&profit_token.address())
                }) {
                    let price_f64 = pool.nominal_price(&weth_token_clone, &profit_token).await.unwrap_or(0.0);

                    let price_u256_scaled = U256::from((price_f64 * 1e18).round() as u128);
                    
                    return (profit_token.address(), Ok(price_u256_scaled));
                } else {
                    return (profit_token.address(), Err(ArbRsError::CalculationError("No liquid WETH pool found for conversion".to_string())));
                }
            }
        });

        for (token_addr, result) in join_all(rate_futs).await {
            if let Ok(rate_u256) = result {
                rate_map.insert(token_addr, rate_u256);
            }
        }
        rate_map
    }

    async fn get_live_gas_price(&self) -> Result<U256, ArbRsError> {
        let gas_price_raw = self.provider.get_gas_price().await?;
        let gas_price_u256: U256 = U256::from(gas_price_raw); 

        Ok(gas_price_u256)
    }

    pub async fn find_opportunities(
        &self,
        block_number: Option<u64>,
    ) -> Vec<ArbitrageSolution<P>> {
        let paths_read_guard = self.cache.paths.read().await;
        let paths: Arc<Vec<Arc<dyn Arbitrage<P>>>> = Arc::new(paths_read_guard.clone());
        
        if paths.is_empty() {
            return Vec::new();
        }

        let mut unique_pools = HashMap::new();
        for path in paths.iter() {
            for pool in path.get_pools() {
                unique_pools.insert(pool.address(), pool.clone());
            }
        }

        tracing::debug!("Found {} unique pools to snapshot.", unique_pools.len());

        let snapshot_futs = unique_pools
            .values()
            .map(|pool| async { (pool.address(), pool.get_snapshot(block_number).await) });

        let snapshot_results = join_all(snapshot_futs).await;

        let mut snapshots = HashMap::new();
        for (address, result) in snapshot_results {
            match result {
                Ok(snapshot) => {
                    snapshots.insert(address, snapshot);
                }
                Err(e) => tracing::warn!(?address, "Failed to get pool snapshot: {:?}", e),
            }
        }

        let live_gas_price = self.get_live_gas_price().await.unwrap_or_else(|e| {
            tracing::warn!("Failed to fetch live gas price: {:?}", e);
            U256::from_limbs([20_000_000_000, 0, 0, 0])
        });

        let path_conversion_rates_map = self.get_all_profit_token_conversion_rates(&paths, &unique_pools).await;

        let paths_clone = paths.clone();
        let snapshots_clone = snapshots;
        let path_conversion_rates_clone = path_conversion_rates_map;

        let task = tokio::task::spawn_blocking(move || {
            let mut opportunities = Vec::new();

            fn build_swap_actions<P>(
                path: &Arc<dyn Arbitrage<P>>,
                start_amount: U256,
                snapshots: &HashMap<Address, PoolSnapshot>,
            ) -> Result<Vec<SwapAction<P>>, ArbRsError>
            where
                P: Provider + Send + Sync + 'static + ?Sized,
            {
                let cycle = path.as_any().downcast_ref::<ArbitrageCycle<P>>().unwrap();
                let mut current_amount = start_amount;
                let mut swap_actions: Vec<SwapAction<P>> = Vec::with_capacity(cycle.path.pools.len());

                const SLIPPAGE_BPS: U256 = U256::from_limbs([5, 0, 0, 0]); 
                const BPS_DENOMINATOR: U256 = U256::from_limbs([10_000, 0, 0, 0]);

                for i in 0..cycle.path.pools.len() {
                    let pool = &cycle.path.pools[i];
                    let token_in = &cycle.path.path[i];
                    let token_out = &cycle.path.path[i + 1];

                    let amount_in_for_hop = current_amount;

                    let exact_amount_out = pool.calculate_tokens_out(
                        token_in, 
                        token_out, 
                        amount_in_for_hop, 
                        snapshots.get(&pool.address()).unwrap()
                    )?;

                    if exact_amount_out.is_zero() {
                        return Err(ArbRsError::CalculationError("Zero output encountered in hop".to_string()));
                    }

                    let min_amount_out = exact_amount_out
                        .checked_mul(BPS_DENOMINATOR.saturating_sub(SLIPPAGE_BPS))
                        .unwrap_or_default()
                        .checked_div(BPS_DENOMINATOR)
                        .unwrap_or_default();

                    swap_actions.push(SwapAction {
                        pool_address: pool.address(),
                        token_in: token_in.clone(),
                        token_out: token_out.clone(),
                        amount_in: amount_in_for_hop,
                        min_amount_out,
                    });

                    current_amount = exact_amount_out;
                }

                Ok(swap_actions)
            }

            const WETH_ADDRESS: Address = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"); 
            const ETHER_SCALE: U256 = U256::from_limbs([1_000_000_000_000_000_000, 0, 0, 0]);
            const BPS_DENOMINATOR: U256 = U256::from_limbs([10_000, 0, 0, 0]);
            const FLASHLOAN_FEE_BPS: U256 = U256::from_limbs([9, 0, 0, 0]); 
            const ESTIMATED_GAS_UNITS: U256 = U256::from_limbs([700_000, 0, 0, 0]);
            const MIN_NET_PROFIT_THRESHOLD: U256 = U256::from_limbs([50_000_000_000_000_000, 0, 0, 0]);

            for (i, path) in paths_clone.iter().enumerate() {
                if !path
                    .get_involved_pools()
                    .iter()
                    .all(|addr| snapshots_clone.contains_key(addr))
                {
                    continue;
                }

                match path.check_viability(&snapshots_clone) {
                    Ok(true) => { /* Continue */ }
                    Ok(false) => {
                        tracing::trace!("Path #{} failed viability check.", i);
                        continue;
                    }
                    Err(e) => {
                        tracing::warn!("Viability check failed for path #{}: {:?}", i, e);
                        continue;
                    }
                }
            
                let cycle = path.as_any().downcast_ref::<ArbitrageCycle<P>>().unwrap();
                let profit_token_address = cycle.path.profit_token.address();

                let gas_cost_weth = ESTIMATED_GAS_UNITS 
                    .checked_mul(live_gas_price)
                    .unwrap_or_default()
                    .checked_div(ETHER_SCALE) 
                    .unwrap_or_default();

                let gas_cost_in_profit_token = if profit_token_address == WETH_ADDRESS {
                    gas_cost_weth
                } else {
                    let conversion_rate_scaled = path_conversion_rates_clone
                        .get(&profit_token_address)
                        .copied()
                        .unwrap_or(ETHER_SCALE);

                    gas_cost_weth
                        .widening_mul(conversion_rate_scaled)
                        .checked_div(ETHER_SCALE.into())
                        .unwrap_or_default().to()
                };

                let optimal_result_input = match optimizer::find_optimal_input(
                    &path,
                    U256::from(10).pow(U256::from(17)), 
                    U256::from(50) * ETHER_SCALE,      
                    &snapshots_clone,
                ) {
                    Ok((opt_input, _)) => opt_input,
                    Err(e) => {
                        tracing::warn!("Optimizer failed for path #{}: {:?}", i, e);
                        continue;
                    }
                };

                let max_capacity_input = match optimizer::find_max_capacity(
                    &path,
                    optimal_result_input, 
                    U256::from(50) * ETHER_SCALE,
                    &snapshots_clone,
                    MIN_NET_PROFIT_THRESHOLD,
                    gas_cost_in_profit_token,
                ) {
                    Ok(cap_input) => cap_input,
                    Err(e) => {
                        tracing::warn!("Capacity search failed for path #{}: {:?}", i, e);
                        continue;
                    }
                };
                
                if max_capacity_input.is_zero() || max_capacity_input < U256::from(10).pow(U256::from(15)) {
                    continue;
                }

                let final_optimal_input = max_capacity_input;

                let gross_profit = path
                    .calculate_out_amount(final_optimal_input, &snapshots_clone)
                    .unwrap_or_default()
                    .saturating_sub(final_optimal_input);

                let flashloan_fee = final_optimal_input 
                    .checked_mul(FLASHLOAN_FEE_BPS)
                    .unwrap_or_default()
                    .checked_div(BPS_DENOMINATOR)
                    .unwrap_or_default();
                
                let total_cost = flashloan_fee.saturating_add(gas_cost_in_profit_token);
                let net_profit = gross_profit.saturating_sub(total_cost);

                if net_profit >= MIN_NET_PROFIT_THRESHOLD { 
                    let swap_actions = match build_swap_actions(
                        &path,
                        final_optimal_input,
                        &snapshots_clone,
                    ) {
                        Ok(actions) => actions,
                        Err(e) => {
                            tracing::warn!("Failed to finalize swap actions for path #{}: {:?}", i, e);
                            continue;
                        }
                    };

                    opportunities.push(ArbitrageSolution {
                        path: path.clone(),
                        optimal_input: final_optimal_input, 
                        gross_profit,
                        net_profit, 
                        swap_actions, 
                    });

                    if let Some(cycle) = path.as_any().downcast_ref::<ArbitrageCycle<P>>() {
                        println!("Profitable path details: {:?}", cycle.path);
                    }

                    println!(
                        "Found profitable opportunity! path_index: {}, NET profit: {}, input: {}",
                        i, net_profit, final_optimal_input
                    );
                }
            }
            opportunities
        });

        let mut opportunities = task.await.unwrap_or_default();
        opportunities.sort_by(|a, b| b.net_profit.cmp(&a.net_profit));

        for (i, opp) in opportunities.iter().enumerate() {
            tracing::info!(
                path_index = i,
                net_profit = ?opp.net_profit,
                input = ?opp.optimal_input,
                "Found profitable opportunity! (Actions: {})",
                opp.swap_actions.len()
            );
        }

        opportunities
    }
}

impl<P: Provider + Send + Sync + 'static + ?Sized> Debug for ArbitrageEngine<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ArbitrageEngine")
            .field("cache", &self.cache)
            .finish()
    }
}

impl<P: Provider + Send + Sync + 'static + ?Sized> Clone for ArbitrageEngine<P> {
    fn clone(&self) -> Self {
        Self {
            cache: self.cache.clone(),
            token_manager: self.token_manager.clone(),
            provider: self.provider.clone(),
        }
    }
}
