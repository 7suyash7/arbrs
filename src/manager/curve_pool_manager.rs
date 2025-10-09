use crate::{
    curve::{attributes_builder, pool::CurveStableswapPool, registry::CurveRegistry},
    db::{DbManager, PoolRecord},
    errors::ArbRsError,
    manager::token_manager::TokenManager,
    pool::LiquidityPool,
};
use alloy_primitives::{Address, address};
use alloy_provider::Provider;
use alloy_rpc_types::{Filter, Log};
use alloy_sol_types::{SolEvent, sol};
use dashmap::DashMap;
use futures::stream::{self, StreamExt};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Mainnet Curve Registry Address
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
    db_manager: Arc<DbManager>,
}

impl<P: Provider + Send + Sync + 'static + ?Sized> CurvePoolManager<P> {
    pub fn new(
        token_manager: Arc<TokenManager<P>>,
        provider: Arc<P>,
        start_block: u64,
        db_manager: Arc<DbManager>,
    ) -> Self {
        let curve_registry = CurveRegistry::new(CURVE_MAINNET_REGISTRY, provider.clone());
        Self {
            token_manager,
            pool_registry: Arc::new(DashMap::new()),
            provider,
            curve_registry,
            last_discovery_block: start_block,
            db_manager,
        }
    }

    pub async fn discover_pools_in_range(
        &self,
        end_block: u64,
    ) -> Result<Vec<Arc<dyn LiquidityPool<P>>>, ArbRsError> {
        if end_block <= self.last_discovery_block {
            return Ok(Vec::new());
        }

        const CHUNK_SIZE: u64 = 10000;
        let mut from_block = self.last_discovery_block + 1;
        let new_pools = Arc::new(Mutex::new(Vec::new()));

        while from_block <= end_block {
            let to_block = (from_block + CHUNK_SIZE - 1).min(end_block);
            println!(
                "[Curve Manager] Discovering pools from {} to {}",
                from_block, to_block
            );

            let event_filter = Filter::new()
                .address(self.curve_registry.address)
                .event_signature(PoolAdded::SIGNATURE_HASH)
                .from_block(from_block)
                .to_block(to_block);

            let logs: Vec<Log> = self.provider.get_logs(&event_filter).await?;

            let provider = self.provider.clone();
            let token_manager = self.token_manager.clone();
            let curve_registry = self.curve_registry.clone();
            let db_manager = self.db_manager.clone();
            let pool_registry = self.pool_registry.clone();
            let new_pools_clone = new_pools.clone();

            stream::iter(logs)
                .for_each_concurrent(5, move |log| {
                    let provider = provider.clone();
                    let token_manager = token_manager.clone();
                    let curve_registry = curve_registry.clone();
                    let db_manager = db_manager.clone();
                    let pool_registry = pool_registry.clone();
                    let new_pools_clone = new_pools_clone.clone();

                    async move {
                        if let Ok(decoded_log) = PoolAdded::decode_log_data(&log.inner.data) {
                            if let Ok(pool) = build_new_discovered_pool(
                                pool_registry,
                                db_manager,
                                token_manager,
                                provider,
                                &curve_registry,
                                decoded_log.pool,
                            )
                            .await
                            {
                                new_pools_clone.lock().await.push(pool);
                            }
                        }
                    }
                })
                .await;

            from_block = to_block + 1;
        }

        let final_pools = Arc::try_unwrap(new_pools).unwrap().into_inner();
        Ok(final_pools)
    }

    pub async fn build_pool_from_record(
        &self,
        record: &PoolRecord,
    ) -> Result<Arc<dyn LiquidityPool<P>>, ArbRsError> {
        if let Some(pool) = self.pool_registry.get(&record.address) {
            return Ok(pool.clone());
        }

        let attributes = if let Some(json_attributes) = &record.attributes_json {
            println!(
                "[CACHE HIT] Loaded Curve attributes for {} from DB.",
                record.address
            );
            serde_json::from_str(json_attributes)
                .map_err(|e| ArbRsError::AbiDecodeError(e.to_string()))?
        } else {
            println!(
                "[CACHE MISS] Fetching Curve attributes for {} from on-chain...",
                record.address
            );
            let tokens: Vec<_> = futures::future::join_all(
                record
                    .tokens
                    .iter()
                    .map(|&addr| self.token_manager.get_token(addr)),
            )
            .await
            .into_iter()
            .collect::<Result<_, _>>()?;

            let fetched_attributes = attributes_builder::build_attributes(
                record.address,
                &tokens,
                self.provider.clone(),
                &self.token_manager,
                &self.curve_registry,
            )
            .await?;

            let json_attributes = serde_json::to_string(&fetched_attributes).unwrap();
            self.db_manager
                .update_pool_attributes(record.address, &json_attributes)
                .await
                .ok();
            println!(
                "[DB SAVE] Saved new Curve attributes for {}.",
                record.address
            );
            fetched_attributes
        };

        let pool = Arc::new(
            CurveStableswapPool::new(
                record.address,
                self.provider.clone(),
                self.token_manager.clone(),
                &self.curve_registry,
                attributes,
            )
            .await?,
        );

        self.pool_registry.insert(record.address, pool.clone());
        Ok(pool)
    }

    pub fn get_all_pools(&self) -> Vec<Arc<dyn LiquidityPool<P>>> {
        self.pool_registry
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }
}

async fn build_new_discovered_pool<P: Provider + Send + Sync + 'static + ?Sized>(
    pool_registry: Arc<PoolRegistry<P>>,
    db_manager: Arc<DbManager>,
    token_manager: Arc<TokenManager<P>>,
    provider: Arc<P>,
    curve_registry: &CurveRegistry<P>,
    pool_address: Address,
) -> Result<Arc<dyn LiquidityPool<P>>, ArbRsError> {
    if pool_registry.contains_key(&pool_address) {
        return Err(ArbRsError::DataFetchError(pool_address));
    }

    println!(
        "[Curve Manager] Building new discovered pool {}",
        pool_address
    );

    let tokens =
        CurveStableswapPool::fetch_coins(&pool_address, provider.clone(), &token_manager).await?;

    let attributes = attributes_builder::build_attributes(
        pool_address,
        &tokens,
        provider.clone(),
        &token_manager,
        curve_registry,
    )
    .await?;

    db_manager
        .save_pool(pool_address, "curve", &tokens, None, None)
        .await
        .ok();

    let json_attributes = serde_json::to_string(&attributes).unwrap();
    db_manager
        .update_pool_attributes(pool_address, &json_attributes)
        .await
        .ok();
    println!(
        "[DB SAVE] Saved new Curve pool and attributes for {}.",
        pool_address
    );

    let pool = Arc::new(
        CurveStableswapPool::new(
            pool_address,
            provider.clone(),
            token_manager.clone(),
            curve_registry,
            attributes,
        )
        .await?,
    );

    pool_registry.insert(pool_address, pool.clone());
    Ok(pool)
}
