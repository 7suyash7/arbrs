use alloy_primitives::{address, Address};
use alloy_provider::{Provider, ProviderBuilder};
use arbrs::{
    arbitrage::{cache::ArbitrageCache, engine::ArbitrageEngine, finder::find_two_pool_cycles}, curve::pool::CurveStableswapPool, db::DbManager, dex::DexVariant, manager::{
        curve_pool_manager::CurvePoolManager, uniswap_v2_pool_manager::UniswapV2PoolManager,
        uniswap_v3_pool_manager::UniswapV3PoolManager,
    }, pool::{uniswap_v3::UniswapV3Pool, LiquidityPool}, TokenManager
};
use std::{sync::Arc, time::Duration};
use url::Url;

const FORK_RPC_URL: &str = "http://127.0.0.1:8545";
const DB_URL: &str = "sqlite:arbrs.db";
const CHAIN_ID: u64 = 1;
const V2_FACTORY_ADDRESS: Address = address!("5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f");
const V3_FACTORY_ADDRESS: Address = address!("1F98431c8aD98523631AE4a59f267346ea31F984");

type DynProvider = dyn Provider + Send + Sync;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting arbrs engine...");

    let db_manager = Arc::new(DbManager::new(DB_URL).await?);
    let known_pools = db_manager.load_all_pools().await?;
    println!("Loaded {} pools from the database.", known_pools.len());

    let url = Url::parse(FORK_RPC_URL)?;
    let provider_arc: Arc<DynProvider> = Arc::new(ProviderBuilder::new().connect_http(url));
    let token_manager = Arc::new(TokenManager::new(provider_arc.clone(), CHAIN_ID));

    let mut last_seen_block = db_manager.get_last_seen_block().await?;
    println!("Starting scan from block {}", last_seen_block);

    let mut v2_pool_manager =
        UniswapV2PoolManager::new(token_manager.clone(), provider_arc.clone(), V2_FACTORY_ADDRESS, last_seen_block);
    let mut v3_pool_manager = UniswapV3PoolManager::new(
        token_manager.clone(),
        provider_arc.clone(),
        CHAIN_ID,
        last_seen_block,
        V3_FACTORY_ADDRESS,
    );
    let mut curve_pool_manager =
        CurvePoolManager::new(token_manager.clone(), provider_arc.clone(), last_seen_block);

    for record in known_pools {
        match record.dex.as_str() {
            "uniswap_v2" => {
                let _ = v2_pool_manager.build_v2_pool(record.address, record.tokens[0], record.tokens[1], DexVariant::UniswapV2).await;
            },
            "uniswap_v3" => {
                if let (Some(fee), Some(tick_spacing)) = (record.fee, record.tick_spacing) {
                    let _ = v3_pool_manager.build_pool(record.address, record.tokens[0], record.tokens[1], fee, tick_spacing).await;
                }
            },
            "curve" => {
                let _ = curve_pool_manager.build_pool(record.address).await;
            },
            _ => {}
        }
    }
    
    let arbitrage_cache = Arc::new(ArbitrageCache::new());
    let arbitrage_engine = ArbitrageEngine::new(arbitrage_cache.clone());
    
    println!("Setup complete. Starting main loop...");

    loop {
        println!("\n--- [ New Cycle ] ---");
        let latest_block = provider_arc.get_block_number().await?;
        println!("Current Block: {}", latest_block);

        println!("Discovering new pools...");
        let (v2_res, v3_res, curve_res) = tokio::join!(
            v2_pool_manager.discover_pools_in_range(latest_block),
            v3_pool_manager.discover_pools_in_range(latest_block),
            curve_pool_manager.discover_pools_in_range(latest_block)
        );

        let v2_discoveries = v2_res?;
        let v3_discoveries = v3_res?;
        let curve_discoveries = curve_res?;
        let new_pools_found = !v2_discoveries.is_empty() || !v3_discoveries.is_empty() || !curve_discoveries.is_empty();

        for pool in &v2_discoveries {
            let tokens = pool.get_all_tokens();
            db_manager.save_pool(pool.address(), "uniswap_v2", &tokens, None, None).await?;
            println!("[DB] Saved V2 pool: {}", pool.address());
        }
        
        for pool in &v3_discoveries {
            if let Some(v3_pool) = pool.as_any().downcast_ref::<UniswapV3Pool<DynProvider>>() {
                let tokens = v3_pool.get_all_tokens();
                db_manager.save_pool(v3_pool.address(), "uniswap_v3", &tokens, Some(v3_pool.fee()), Some(v3_pool.tick_spacing())).await?;
                println!("[DB] Saved V3 pool: {}", v3_pool.address());
            }
        }

        for pool in &curve_discoveries {
            if let Some(curve_pool) = pool.as_any().downcast_ref::<CurveStableswapPool<DynProvider>>() {
                let tokens = curve_pool.get_all_tokens();
                db_manager.save_pool(curve_pool.address(), "curve", &tokens, None, None).await?;
                println!("[DB] Saved Curve pool: {}", curve_pool.address());
            }
        }

        if new_pools_found {
            println!("New pools found, updating arbitrage paths...");
            let new_paths = find_two_pool_cycles(&v2_pool_manager, &v3_pool_manager, &curve_pool_manager);
            println!("Found {} potential 2-pool arbitrage paths.", new_paths.len());
            for path in new_paths {
                arbitrage_cache.add_path(path).await;
            }
        } else {
            println!("No new pools found.");
        }

        arbitrage_engine.calculate_all_paths(Some(latest_block)).await;

        db_manager.update_last_seen_block(latest_block).await?;
        println!("[DB] Updated last seen block to {}", latest_block);

        println!("--- [ Cycle Complete ] ---");
        tokio::time::sleep(Duration::from_secs(12)).await;
    }
}
