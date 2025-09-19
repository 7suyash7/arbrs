use crate::dex::{DexDetails, DexVariant, build_mainnet_dex_registry};
use crate::errors::ArbRsError;
use crate::manager::pool_discovery::discover_new_v2_pools;
use crate::manager::token_manager::TokenManager;
use crate::pool::LiquidityPool;
use alloy_primitives::Address;
use alloy_provider::Provider;
use dashmap::DashMap;
use std::collections::HashMap;
use std::sync::Arc;

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
        println!(
            "[discover_pools_in_range] Current last_discovery_block: {}",
            self.last_discovery_block
        );
        if end_block <= self.last_discovery_block {
            println!(
                "[discover_pools_in_range] No new blocks to scan. end_block: {}, last_discovery_block: {}",
                end_block, self.last_discovery_block
            );
            return Ok(Vec::new());
        }

        let from_block = self.last_discovery_block + 1;
        println!(
            "[discover_pools_in_range] Discovering pools from {} to {}",
            from_block, end_block
        );

        let discovered_pools_data = discover_new_v2_pools(
            self.provider.clone(),
            self.factory_address,
            from_block,
            end_block,
        )
        .await?;

        println!(
            "[discover_pools_in_range] Discovered {} new pools.",
            discovered_pools_data.len()
        );

        let mut new_pools = Vec::new();

        for pool_data in discovered_pools_data {
            println!(
                "[discover_pools_in_range] Building pool at address: {}",
                pool_data.pool_address
            );
            let pool = self
                .build_v2_pool(
                    pool_data.pool_address,
                    pool_data.token0,
                    pool_data.token1,
                    DexVariant::UniswapV2,
                )
                .await?;
            new_pools.push(pool);
        }

        self.last_discovery_block = end_block;
        Ok(new_pools)
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
}
