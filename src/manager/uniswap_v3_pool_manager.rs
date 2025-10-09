use crate::errors::ArbRsError;
use crate::manager::pool_discovery::discover_new_v3_pools;
use crate::manager::token_manager::TokenManager;
use crate::pool::{
    LiquidityPool, uniswap_v3::UniswapV3Pool, uniswap_v3_snapshot::UniswapV3LiquiditySnapshot,
};
use alloy_primitives::Address;
use alloy_provider::Provider;
use dashmap::DashMap;
use futures::{StreamExt, stream};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

type PoolRegistry<P> = DashMap<Address, Arc<dyn LiquidityPool<P>>>;

pub struct UniswapV3PoolManager<P: Provider + Send + Sync + 'static + ?Sized> {
    token_manager: Arc<TokenManager<P>>,
    pool_registry: Arc<PoolRegistry<P>>,
    provider: Arc<P>,
    liquidity_snapshot: Arc<RwLock<UniswapV3LiquiditySnapshot<P>>>,
    factory_address: Address,
    pub last_discovery_block: u64,
}

impl<P: Provider + Send + Sync + 'static + ?Sized> UniswapV3PoolManager<P> {
    pub fn new(
        token_manager: Arc<TokenManager<P>>,
        provider: Arc<P>,
        chain_id: u64,
        start_block: u64,
        factory_address: Address,
    ) -> Self {
        Self {
            token_manager,
            pool_registry: Arc::new(DashMap::new()),
            provider: provider.clone(),
            liquidity_snapshot: Arc::new(RwLock::new(UniswapV3LiquiditySnapshot::new(
                provider,
                chain_id,
                start_block,
            ))),
            factory_address,
            last_discovery_block: start_block,
        }
    }

    pub async fn build_pool(
        &self,
        pool_address: Address,
        token_a: Address,
        token_b: Address,
        fee: u32,
        tick_spacing: i32,
    ) -> Result<Arc<dyn LiquidityPool<P>>, ArbRsError> {
        if let Some(pool) = self.pool_registry.get(&pool_address) {
            return Ok(pool.clone());
        }

        let initial_liquidity_map = {
            let snapshot = self.liquidity_snapshot.read().await;
            snapshot.liquidity_snapshot.get(&pool_address).cloned()
        };

        let token0 = self
            .token_manager
            .get_token(if token_a < token_b { token_a } else { token_b })
            .await?;
        let token1 = self
            .token_manager
            .get_token(if token_a < token_b { token_b } else { token_a })
            .await?;

        let pool = Arc::new(UniswapV3Pool::new(
            pool_address,
            token0,
            token1,
            fee,
            tick_spacing,
            self.provider.clone(),
            initial_liquidity_map,
        ));

        let pending_updates = {
            let mut snapshot = self.liquidity_snapshot.write().await;
            snapshot.pending_updates(pool_address)
        };

        for update in pending_updates {
            pool.update_liquidity_map(update).await;
        }

        self.pool_registry.insert(pool_address, pool.clone());
        Ok(pool)
    }

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
                "[V3 Manager] Discovering pools from block {} to {}",
                from_block, to_block
            );

            let discovered_pools_data = discover_new_v3_pools(
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
            let liquidity_snapshot_clone = self.liquidity_snapshot.clone();

            stream::iter(discovered_pools_data)
                .for_each_concurrent(CONCURRENT_BUILDS, |pool_data| {
                    let token_manager = token_manager_clone.clone();
                    let provider = provider_clone.clone();
                    let pool_registry = pool_registry_clone.clone();
                    let liquidity_snapshot = liquidity_snapshot_clone.clone();
                    let new_pools = new_pools_in_chunk.clone();

                    async move {
                        if let Ok(pool) = build_and_register_v3_pool(
                            pool_registry,
                            token_manager,
                            provider,
                            liquidity_snapshot,
                            pool_data.pool_address,
                            pool_data.token0,
                            pool_data.token1,
                            pool_data.fee,
                            pool_data.tick_spacing,
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

    pub fn get_all_pools(&self) -> Vec<Arc<dyn LiquidityPool<P>>> {
        self.pool_registry
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }
}

async fn build_and_register_v3_pool<P: Provider + Send + Sync + 'static + ?Sized>(
    pool_registry: Arc<PoolRegistry<P>>,
    token_manager: Arc<TokenManager<P>>,
    provider: Arc<P>,
    liquidity_snapshot: Arc<RwLock<UniswapV3LiquiditySnapshot<P>>>,
    pool_address: Address,
    token_a: Address,
    token_b: Address,
    fee: u32,
    tick_spacing: i32,
) -> Result<Arc<dyn LiquidityPool<P>>, ArbRsError> {
    if let Some(pool) = pool_registry.get(&pool_address) {
        return Ok(pool.clone());
    }

    let initial_liquidity_map = {
        let snapshot = liquidity_snapshot.read().await;
        snapshot.liquidity_snapshot.get(&pool_address).cloned()
    };

    let token0 = token_manager
        .get_token(if token_a < token_b { token_a } else { token_b })
        .await?;
    let token1 = token_manager
        .get_token(if token_a < token_b { token_b } else { token_a })
        .await?;

    let pool = Arc::new(UniswapV3Pool::new(
        pool_address,
        token0,
        token1,
        fee,
        tick_spacing,
        provider,
        initial_liquidity_map,
    ));

    let pending_updates = {
        let mut snapshot = liquidity_snapshot.write().await;
        snapshot.pending_updates(pool_address)
    };

    for update in pending_updates {
        pool.update_liquidity_map(update).await;
    }

    pool_registry.insert(pool_address, pool.clone());
    Ok(pool)
}
