use crate::dex::{DexDetails, DexVariant, build_mainnet_dex_registry};
use crate::errors::ArbRsError;
use crate::manager::token_manager::TokenManager;
use crate::pool::LiquidityPool;
use crate::pool::strategy::{PancakeV2Logic, StandardV2Logic};
use crate::pool::uniswap_v2::UniswapV2Pool;
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
}

impl<P: Provider + Send + Sync + 'static + ?Sized> UniswapV2PoolManager<P> {
    pub fn new(token_manager: Arc<TokenManager<P>>, provider: Arc<P>) -> Self {
        Self {
            token_manager,
            _dex_registry: build_mainnet_dex_registry(),
            pool_registry: Arc::new(DashMap::new()),
            provider,
        }
    }

    /// Creates or retrieves a cached V2 liquidity pool instance using an explicit DEX type.
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
                let strategy = StandardV2Logic;
                Arc::new(UniswapV2Pool::new(
                    pool_address,
                    token0,
                    token1,
                    self.provider.clone(),
                    strategy,
                ))
            }
            DexVariant::PancakeSwapV2 => {
                let strategy = PancakeV2Logic;
                Arc::new(UniswapV2Pool::new(
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
}
