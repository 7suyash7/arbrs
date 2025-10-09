use crate::{
    balancer::pool::BalancerPool, db::DbManager, errors::ArbRsError,
    manager::token_manager::TokenManager, pool::LiquidityPool,
};
use alloy_primitives::{Address, U256, address};
use alloy_provider::Provider;
use alloy_rpc_types::{Filter, Log};
use alloy_sol_types::{SolEvent, sol};
use dashmap::DashMap;
use futures::stream::{self, StreamExt};
use std::sync::Arc;
use tokio::sync::Mutex;

// The official Balancer V2 Vault address on Mainnet
const BALANCER_V2_VAULT: Address = address!("BA12222222228d8Ba445958a75a0704d566BF2C8");

sol! {
    event PoolRegistered(bytes32 indexed poolId, address indexed poolAddress, uint256 specialization);
}

type PoolRegistry<P> = DashMap<Address, Arc<dyn LiquidityPool<P>>>;

/// Manages the discovery and lifecycle of Balancer pools.
pub struct BalancerPoolManager<P: Provider + Send + Sync + 'static + ?Sized> {
    token_manager: Arc<TokenManager<P>>,
    pool_registry: Arc<PoolRegistry<P>>,
    provider: Arc<P>,
    db_manager: Arc<DbManager>,
    last_discovery_block: u64,
}

impl<P: Provider + Send + Sync + 'static + ?Sized> BalancerPoolManager<P> {
    /// Creates a new `BalancerPoolManager`.
    pub fn new(
        token_manager: Arc<TokenManager<P>>,
        provider: Arc<P>,
        db_manager: Arc<DbManager>,
        start_block: u64,
    ) -> Self {
        Self {
            token_manager,
            pool_registry: Arc::new(DashMap::new()),
            provider,
            db_manager,
            last_discovery_block: start_block,
        }
    }

    /// Hydrates a pool from a database record.
    pub async fn build_pool(
        &self,
        address: Address,
    ) -> Result<Arc<dyn LiquidityPool<P>>, ArbRsError> {
        if let Some(pool) = self.pool_registry.get(&address) {
            return Ok(pool.clone());
        }

        tracing::debug!(?address, "Hydrating Balancer pool from DB");

        let pool = Arc::new(
            BalancerPool::new(
                address,
                self.provider.clone(),
                self.token_manager.clone(),
                self.db_manager.clone(),
            )
            .await?,
        );

        self.pool_registry.insert(address, pool.clone());
        tracing::debug!(?address, "Successfully hydrated and cached Balancer pool.");

        Ok(pool)
    }

    /// Discovers new Balancer pools within a specified block range by listening for `PoolRegistered` events.
    pub async fn discover_pools_in_range(
        &mut self,
        end_block: u64,
    ) -> Result<Vec<Arc<dyn LiquidityPool<P>>>, ArbRsError> {
        if end_block <= self.last_discovery_block {
            return Ok(Vec::new());
        }

        const CHUNK_SIZE: u64 = 25000; // Balancer events can be sparse, larger chunk is ok
        let mut from_block = self.last_discovery_block + 1;
        let new_pools = Arc::new(Mutex::new(Vec::new()));

        while from_block <= end_block {
            let to_block = (from_block + CHUNK_SIZE - 1).min(end_block);
            tracing::info!(
                "[Balancer Manager] Discovering pools from {} to {}",
                from_block,
                to_block
            );

            let event_filter = Filter::new()
                .address(BALANCER_V2_VAULT)
                .event_signature(PoolRegistered::SIGNATURE_HASH)
                .from_block(from_block)
                .to_block(to_block);

            let logs: Vec<Log> = self.provider.get_logs(&event_filter).await?;

            let build_tasks = logs.into_iter().map(|log| {
                let pool_registry = self.pool_registry.clone();
                let db_manager = self.db_manager.clone();
                let token_manager = self.token_manager.clone();
                let provider = self.provider.clone();

                async move {
                    if let Ok(decoded_log) = PoolRegistered::decode_log_data(&log.inner.data) {
                        // We are only interested in Weighted Pools for now (specialization == 0)
                        if decoded_log.specialization == U256::ZERO {
                            match build_new_discovered_pool(
                                pool_registry,
                                db_manager,
                                token_manager,
                                provider,
                                decoded_log.poolAddress,
                            )
                            .await
                            {
                                Ok(pool) => return Some(pool),
                                Err(e) => tracing::warn!(
                                    "Failed to build discovered Balancer pool {}: {:?}",
                                    decoded_log.poolAddress,
                                    e
                                ),
                            }
                        }
                    }
                    None
                }
            });

            let results = stream::iter(build_tasks)
                .buffer_unordered(10)
                .collect::<Vec<_>>()
                .await;

            let mut guard = new_pools.lock().await;
            for pool_opt in results {
                if let Some(pool) = pool_opt {
                    guard.push(pool);
                }
            }

            from_block = to_block + 1;
        }

        self.last_discovery_block = end_block;
        let final_pools = Arc::try_unwrap(new_pools).unwrap().into_inner();
        Ok(final_pools)
    }

    /// Returns a vector of all pools currently in the manager's registry.
    pub fn get_all_pools(&self) -> Vec<Arc<dyn LiquidityPool<P>>> {
        self.pool_registry
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }
}

/// Helper function to build a newly discovered pool, save it to the DB, and register it.
async fn build_new_discovered_pool<P: Provider + Send + Sync + 'static + ?Sized>(
    pool_registry: Arc<PoolRegistry<P>>,
    db_manager: Arc<DbManager>,
    token_manager: Arc<TokenManager<P>>,
    provider: Arc<P>,
    pool_address: Address,
) -> Result<Arc<dyn LiquidityPool<P>>, ArbRsError> {
    if pool_registry.contains_key(&pool_address) {
        return Err(ArbRsError::DataFetchError(pool_address));
    }

    tracing::info!("[Balancer Manager] New pool discovered: {}", pool_address);

    let pool = Arc::new(
        BalancerPool::new(
            pool_address,
            provider,
            token_manager.clone(),
            db_manager.clone(),
        )
        .await?,
    );

    db_manager
        .save_pool(pool_address, "balancer", &pool.get_all_tokens(), None, None)
        .await
        .unwrap_or_else(|e| {
            tracing::error!(
                "Failed to save new Balancer pool {} to DB: {:?}",
                pool_address,
                e
            );
        });

    pool_registry.insert(pool_address, pool.clone());
    Ok(pool)
}
