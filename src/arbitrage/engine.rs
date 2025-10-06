use crate::arbitrage::{cache::ArbitrageCache, cycle::ArbitrageCycle, optimizer, types::ProfitableOpportunity};
use alloy_primitives::U256;
use alloy_provider::Provider;
use futures::future::join_all;
use std::{
    collections::HashMap,
    fmt::{self, Debug},
    sync::Arc,
};

/// The main engine responsible for evaluating arbitrage opportunities.
pub struct ArbitrageEngine<P: Provider + Send + Sync + 'static + ?Sized> {
    pub cache: Arc<ArbitrageCache<P>>,
}

impl<P: Provider + Send + Sync + 'static + ?Sized> ArbitrageEngine<P> {
    pub fn new(cache: Arc<ArbitrageCache<P>>) -> Self {
        Self { cache }
    }

    pub async fn find_opportunities(
        &self,
        block_number: Option<u64>,
    ) -> Vec<ProfitableOpportunity<P>> {
        let paths = self.cache.paths.read().await;
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

        let snapshot_futs = unique_pools.values().map(|pool| async {
            (pool.address(), pool.get_snapshot(block_number).await)
        });
        
        let snapshot_results = join_all(snapshot_futs).await;

        let mut snapshots = HashMap::new();
        for (address, result) in snapshot_results {
            match result {
                Ok(snapshot) => { snapshots.insert(address, snapshot); },
                Err(e) => tracing::warn!(?address, "Failed to get pool snapshot: {:?}", e),
            }
        }

        let paths_clone = paths.clone();
        let task = tokio::task::spawn_blocking(move || {
            let mut opportunities = Vec::new();

            for (i, path) in paths_clone.iter().enumerate() {
                if !path.get_involved_pools().iter().all(|addr| snapshots.contains_key(addr)) {
                    continue;
                }

                match path.check_viability(&snapshots) {
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

                match optimizer::find_optimal_input(&path, U256::from(10).pow(U256::from(17)), U256::from(50) * U256::from(10).pow(U256::from(18)), &snapshots) {
                    Ok((optimal_input, gross_profit)) => {
                        if gross_profit > U256::ZERO {
                            opportunities.push(ProfitableOpportunity {
                                path: path.clone(),
                                optimal_input,
                                gross_profit,
                            });
                            
                            // Use the correct variable `path` to downcast
                            if let Some(cycle) = path.as_any().downcast_ref::<ArbitrageCycle<P>>() {
                                println!("Profitable path details: {:?}", cycle.path);
                            }
                            
                            // Use the correct `println!` syntax with {} placeholders
                            println!(
                                "Found profitable opportunity! path_index: {}, profit: {}, input: {}",
                                i, gross_profit, optimal_input
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Optimizer failed for path #{}: {:?}", i, e);
                    }
                }
            }
            opportunities
        });

        let mut opportunities = task.await.unwrap_or_default();
        opportunities.sort_by(|a, b| b.gross_profit.cmp(&a.gross_profit));

        for (i, opp) in opportunities.iter().enumerate() {
            tracing::info!(
                path_index = i,
                profit = ?opp.gross_profit,
                input = ?opp.optimal_input,
                "Found profitable opportunity!"
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
        }
    }
}