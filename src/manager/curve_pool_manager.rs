use crate::curve::pool::{self, CurveStableswapPool};
use crate::curve::registry::CurveRegistry;
use crate::errors::ArbRsError;
use crate::manager::token_manager::TokenManager;
use crate::pool::LiquidityPool;
use alloy_primitives::{Address, address};
use alloy_provider::Provider;
use alloy_rpc_types::{Filter, Log};
use alloy_sol_types::{SolEvent, sol};
use dashmap::DashMap;
use std::sync::Arc;

// Mainnet Curve Registry Address from degenbot
const CURVE_MAINNET_REGISTRY: Address = address!("90E00ACe148ca3b23Ac1bC8C240C2a7Dd9c2d7f5");

sol! {
    event PoolAdded(address indexed pool);
}

type PoolRegistry<P> = DashMap<Address, Arc<dyn LiquidityPool<P>>>;

pub struct CurvePoolManager<P: Provider + Send + Sync + 'static + ?Sized> {
    token_manager: Arc<TokenManager<P>>,
    pool_registry: Arc<PoolRegistry<P>>,
    provider: Arc<P>,
    curve_registry: CurveRegistry<P>,
    pub last_discovery_block: u64,
}

impl<P: Provider + Send + Sync + 'static + ?Sized> CurvePoolManager<P> {
    pub fn new(token_manager: Arc<TokenManager<P>>, provider: Arc<P>, start_block: u64) -> Self {
        let curve_registry = CurveRegistry::new(CURVE_MAINNET_REGISTRY, provider.clone());
        Self {
            token_manager,
            pool_registry: Arc::new(DashMap::new()),
            provider,
            curve_registry,
            last_discovery_block: start_block,
        }
    }

    /// Discovers new Curve pools from the last discovery block up to the latest block.
    pub async fn discover_pools(&mut self) -> Result<Vec<Arc<dyn LiquidityPool<P>>>, ArbRsError> {
        let latest_block = self
            .provider
            .get_block_number()
            .await
            .map_err(|e| ArbRsError::ProviderError(e.to_string()))?;
        self.discover_pools_in_range(latest_block).await
    }

    /// Discovers new pools within a specified block range and adds them to the manager.
    pub async fn discover_pools_in_range(
        &mut self,
        end_block: u64,
    ) -> Result<Vec<Arc<dyn LiquidityPool<P>>>, ArbRsError> {
        if end_block <= self.last_discovery_block {
            return Ok(Vec::new());
        }

        let from_block = self.last_discovery_block + 1;
        println!(
            "[CurvePoolManager] Discovering pools from {} to {}",
            from_block, end_block
        );

        let event_filter = Filter::new()
            .address(self.curve_registry.address)
            .event_signature(PoolAdded::SIGNATURE_HASH)
            .from_block(from_block)
            .to_block(end_block);

        let logs: Vec<Log> = self
            .provider
            .get_logs(&event_filter)
            .await
            .map_err(|e| ArbRsError::ProviderError(e.to_string()))?;

        println!(
            "[CurvePoolManager] Discovered {} new pools via events.",
            logs.len()
        );

        let mut new_pools = Vec::new();
        for log in logs {
            match PoolAdded::decode_log(&log.inner) {
                Ok(decoded_log) => {
                    let pool_address = decoded_log.pool;
                    if let Ok(pool) = self.build_pool(pool_address).await {
                        new_pools.push(pool);
                    }
                }
                Err(e) => {
                    eprintln!("[CurvePoolManager] FAILED to decode PoolAdded log: {:?}", e);
                }
            }
        }

        self.last_discovery_block = end_block;
        Ok(new_pools)
    }

    /// Creates a new CurveStableswapPool instance and adds it to the registry.
    pub async fn build_pool(
        &self,
        pool_address: Address,
    ) -> Result<Arc<dyn LiquidityPool<P>>, ArbRsError> {
        if let Some(pool) = self.pool_registry.get(&pool_address) {
            return Ok(pool.clone());
        }

        println!(
            "[CurvePoolManager] Building pool at address: {}",
            pool_address
        );

        let pool = Arc::new(
            CurveStableswapPool::new(
                pool_address,
                self.provider.clone(),
                &self.token_manager,
                &self.curve_registry,
            )
            .await?,
        );

        self.pool_registry.insert(pool_address, pool.clone());
        Ok(pool)
    }

    /// Retrieves a pool from the registry by its address.
    pub fn get_pool_by_address(&self, address: Address) -> Option<Arc<dyn LiquidityPool<P>>> {
        self.pool_registry.get(&address).map(|pool| pool.clone())
    }
}
