use crate::dex::{DexDetails, DexVariant, build_mainnet_dex_registry};
use crate::errors::ArbRsError;
use crate::manager::pool_discovery::discover_new_v2_pools;
use crate::manager::token_manager::TokenManager;
use crate::pool::LiquidityPool;
use alloy_primitives::Address;
use alloy_provider::Provider;
use dashmap::DashMap;
use futures::{StreamExt, stream};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

type PoolRegistry<P> = DashMap<Address, Arc<dyn LiquidityPool<P>>>;

pub struct UniswapV2PoolManager<P: Provider + Send + Sync + 'static + ?Sized> {
    token_manager: Arc<TokenManager<P>>,
    _dex_registry: HashMap<Address, DexDetails>,
    pool_registry: Arc<PoolRegistry<P>>,
    provider: Arc<P>,
    factory_address: Address,
    pub last_discovery_block: u64,
}

impl<P: Provider + Send + Sync + 'static + ?Sized> UniswapV2PoolManager<P> {
    pub fn new(
        token_manager: Arc<TokenManager<P>>,
        provider: Arc<P>,
        factory_address: Address,
        start_block: u64,
    ) -> Self {
        Self {
            token_manager,
            pool_registry: Arc::new(DashMap::new()),
            _dex_registry: build_mainnet_dex_registry(),
            provider,
            factory_address,
            last_discovery_block: start_block,
        }
    }

    /// Discovers new pools within a specified block range and adds them to the manager.
    pub async fn discover_pools_in_range(
        &mut self,
        end_block: u64,
    ) -> Result<Vec<Arc<dyn LiquidityPool<P>>>, ArbRsError> {
        if end_block <= self.last_discovery_block {
            return Ok(Vec::new());
        }

        const CHUNK_SIZE: u64 = 10000;
        let mut from_block = self.last_discovery_block + 1;
        let mut all_new_pools = Vec::new();

        while from_block <= end_block {
            let to_block = (from_block + CHUNK_SIZE - 1).min(end_block);
            println!(
                "[V2 Manager] Discovering pools from block {} to {}",
                from_block, to_block
            );

            let discovered_pools_data = discover_new_v2_pools(
                self.provider.clone(),
                self.factory_address,
                from_block,
                to_block,
            )
            .await?;

            const CONCURRENT_BUILDS: usize = 5;

            let new_pools_in_chunk = Arc::new(Mutex::new(Vec::new()));

            let token_manager_clone = self.token_manager.clone();
            let provider_clone = self.provider.clone();
            let pool_registry_clone = self.pool_registry.clone();

            stream::iter(discovered_pools_data)
                .for_each_concurrent(CONCURRENT_BUILDS, |pool_data| {
                    let token_manager = token_manager_clone.clone();
                    let provider = provider_clone.clone();
                    let pool_registry = pool_registry_clone.clone();
                    let new_pools = new_pools_in_chunk.clone();

                    async move {
                        if let Ok(pool) = build_and_register_v2_pool(
                            pool_registry,
                            token_manager,
                            provider,
                            pool_data.pool_address,
                            pool_data.token0,
                            pool_data.token1,
                            DexVariant::UniswapV2,
                        )
                        .await
                        {
                            let mut new_pools_guard = new_pools.lock().await;
                            new_pools_guard.push(pool);
                        }
                    }
                })
                .await;

            let new_pools = Arc::try_unwrap(new_pools_in_chunk).unwrap().into_inner();
            all_new_pools.extend(new_pools);

            from_block = to_block + 1;
        }

        self.last_discovery_block = end_block;
        Ok(all_new_pools)
    }

    /// Discovers new pools from the last discovered block up to the latest block.
    pub async fn discover_pools(&mut self) -> Result<Vec<Arc<dyn LiquidityPool<P>>>, ArbRsError> {
        let latest_block = self
            .provider
            .get_block_number()
            .await
            .map_err(|e| ArbRsError::ProviderError(e.to_string()))?;
        self.discover_pools_in_range(latest_block).await
    }

    /// Creates or retrieves a cached V2 liquidity pool instance.
    pub async fn build_v2_pool(
        &self,
        pool_address: Address,
        token_a: Address,
        token_b: Address,
        dex_type: DexVariant,
    ) -> Result<Arc<dyn LiquidityPool<P>>, ArbRsError> {
        if let Some(pool) = self.pool_registry.get(&pool_address) {
            return Ok(pool.clone());
        }

        let token0 = self
            .token_manager
            .get_token(if token_a < token_b { token_a } else { token_b })
            .await?;
        let token1 = self
            .token_manager
            .get_token(if token_a < token_b { token_b } else { token_a })
            .await?;

        let pool: Arc<dyn LiquidityPool<P>> = match dex_type {
            DexVariant::UniswapV2 | DexVariant::SushiSwap => {
                let strategy = crate::pool::strategy::StandardV2Logic;
                Arc::new(crate::pool::uniswap_v2::UniswapV2Pool::new(
                    pool_address,
                    token0,
                    token1,
                    self.provider.clone(),
                    strategy,
                ))
            }
            DexVariant::PancakeSwapV2 => {
                let strategy = crate::pool::strategy::PancakeV2Logic;
                Arc::new(crate::pool::uniswap_v2::UniswapV2Pool::new(
                    pool_address,
                    token0,
                    token1,
                    self.provider.clone(),
                    strategy,
                ))
            }
        };

        self.pool_registry.insert(pool_address, pool.clone());
        Ok(pool)
    }

    /// Retrieves a pool from the registry by its address.
    pub fn get_pool_by_address(&self, address: Address) -> Option<Arc<dyn LiquidityPool<P>>> {
        self.pool_registry.get(&address).map(|pool| pool.clone())
    }

    pub fn get_all_pools(&self) -> Vec<Arc<dyn LiquidityPool<P>>> {
        self.pool_registry
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }
}

async fn build_and_register_v2_pool<P: Provider + Send + Sync + 'static + ?Sized>(
    pool_registry: Arc<PoolRegistry<P>>,
    token_manager: Arc<TokenManager<P>>,
    provider: Arc<P>,
    pool_address: Address,
    token_a: Address,
    token_b: Address,
    dex_type: DexVariant,
) -> Result<Arc<dyn LiquidityPool<P>>, ArbRsError> {
    if let Some(pool) = pool_registry.get(&pool_address) {
        return Ok(pool.clone());
    }

    let token0 = token_manager
        .get_token(if token_a < token_b { token_a } else { token_b })
        .await?;
    let token1 = token_manager
        .get_token(if token_a < token_b { token_b } else { token_a })
        .await?;

    let pool: Arc<dyn LiquidityPool<P>> = match dex_type {
        DexVariant::UniswapV2 | DexVariant::SushiSwap => {
            Arc::new(crate::pool::uniswap_v2::UniswapV2Pool::new(
                pool_address,
                token0,
                token1,
                provider,
                crate::pool::strategy::StandardV2Logic,
            ))
        }
        DexVariant::PancakeSwapV2 => Arc::new(crate::pool::uniswap_v2::UniswapV2Pool::new(
            pool_address,
            token0,
            token1,
            provider,
            crate::pool::strategy::PancakeV2Logic,
        )),
    };

    pool_registry.insert(pool_address, pool.clone());
    Ok(pool)
}
