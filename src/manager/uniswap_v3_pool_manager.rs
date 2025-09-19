use crate::errors::ArbRsError;
use crate::manager::pool_discovery::discover_new_v3_pools;
use crate::manager::token_manager::TokenManager;
use crate::pool::{
    LiquidityPool, uniswap_v3::UniswapV3Pool, uniswap_v3_snapshot::UniswapV3LiquiditySnapshot,
};
use alloy_primitives::Address;
use alloy_provider::Provider;
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

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

        let discovered_pools_data = discover_new_v3_pools(
            self.provider.clone(),
            self.factory_address,
            self.last_discovery_block + 1,
            end_block,
        )
        .await?;

        let mut new_pools = Vec::new();
        for pool_data in discovered_pools_data {
            let pool = self
                .build_pool(
                    pool_data.pool_address,
                    pool_data.token0,
                    pool_data.token1,
                    pool_data.fee,
                    pool_data.tick_spacing,
                )
                .await?;
            new_pools.push(pool);
        }

        self.last_discovery_block = end_block;
        Ok(new_pools)
    }
}
