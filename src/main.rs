use alloy_primitives::{address, Address};
use alloy_provider::{Provider, ProviderBuilder};
use alloy_transport_ws::WsConnect;
use arbrs::{
    arbitrage::{cache::ArbitrageCache, engine::ArbitrageEngine, finder::{find_three_pool_cycles, find_two_pool_cycles}},
    db::DbManager,
    manager::{
        curve_pool_manager::CurvePoolManager, uniswap_v2_pool_manager::UniswapV2PoolManager,
        uniswap_v3_pool_manager::UniswapV3PoolManager,
    },
    TokenManager,
};
use futures::stream::StreamExt;
use std::sync::Arc;

// const FORK_RPC_URL: &str = "ws://127.0.0.1:8545";
const FORK_RPC_URL: &str = "wss://mainnet.infura.io/ws/v3/";
const DB_URL: &str = "sqlite:arbrs.db";
const CHAIN_ID: u64 = 1;
const V2_FACTORY_ADDRESS: Address = address!("5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f");
const V3_FACTORY_ADDRESS: Address = address!("1F98431c8aD98523631AE4a59f267346ea31F984");

type DynProvider = dyn Provider + Send + Sync;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    tracing::info!("Starting arbrs engine...");
    println!("Starting arbrs engine...");

    let db_manager = Arc::new(DbManager::new(DB_URL).await?);
    let known_pools = db_manager.load_all_pools().await?;
    println!("Loaded {} pools from the database.", known_pools.len());

    let ws = WsConnect::new(FORK_RPC_URL);
    let provider = ProviderBuilder::new().connect_ws(ws).await?;

    let mut stream = provider.subscribe_blocks().await?.into_stream();
    let provider_arc: Arc<DynProvider> = Arc::new(provider);
    let token_manager = Arc::new(TokenManager::new(provider_arc.clone(), CHAIN_ID, db_manager.clone()));

    let mut last_seen_block = provider_arc.get_block_number().await?;
    let mut v2_pool_manager =
        UniswapV2PoolManager::new(token_manager.clone(), provider_arc.clone(), V2_FACTORY_ADDRESS, last_seen_block);
    let mut v3_pool_manager = UniswapV3PoolManager::new(
        token_manager.clone(), provider_arc.clone(), CHAIN_ID, last_seen_block, V3_FACTORY_ADDRESS,
    );
    let curve_pool_manager =
        CurvePoolManager::new(token_manager.clone(), provider_arc.clone(), last_seen_block, db_manager.clone());

   tracing::info!("Hydrating pool managers from database...");
    let mut successful_hydrations = 0;
    for record in &known_pools {
        tracing::debug!(address = ?record.address, dex = ?record.dex, "Processing record");

        let hydration_result = match record.dex.to_lowercase().as_str() {
            "uniswap v2" => {
                v2_pool_manager.build_v2_pool(record.address, record.tokens[0], record.tokens[1], arbrs::dex::DexVariant::UniswapV2).await
            },
            "uniswap v3" => {
                if let (Some(fee), Some(tick_spacing)) = (record.fee, record.tick_spacing) {
                    v3_pool_manager.build_pool(record.address, record.tokens[0], record.tokens[1], fee, tick_spacing).await
                } else {
                    tracing::warn!(?record.address, "Skipping V3 pool due to missing fee/tick_spacing");
                    continue;
                }
            },
            "curve" => {
                curve_pool_manager.build_pool_from_record(record).await
            },
            unrecognized_dex => {
                tracing::trace!(dex = unrecognized_dex, "Skipping unrecognized dex type");
                continue;
            }
        };

        match hydration_result {
            Ok(_) => {
                successful_hydrations += 1;
                tracing::debug!(?record.address, "Successfully hydrated pool.");
            }
            Err(e) => {
                tracing::warn!(?record.address, "Failed to hydrate pool: {:?}", e);
            }
        }
    }
    tracing::info!("Successfully hydrated {} of {} pools.", successful_hydrations, known_pools.len());

    let arbitrage_cache = Arc::new(ArbitrageCache::new());
    let arbitrage_engine = ArbitrageEngine::new(arbitrage_cache.clone());
    
    println!("Finding initial arbitrage paths...");
    let initial_paths_2_pool = find_two_pool_cycles(&v2_pool_manager, &v3_pool_manager, &curve_pool_manager);
    let initial_paths_3_pool = find_three_pool_cycles(&v2_pool_manager, &v3_pool_manager, &curve_pool_manager, &token_manager).await;
    let mut initial_paths = initial_paths_2_pool;
    initial_paths.extend(initial_paths_3_pool);

    println!("Found {} potential 2- and 3-pool arbitrage paths.", initial_paths.len());
    for path in initial_paths {
        arbitrage_cache.add_path(path).await;
    }

    println!("Setup complete. Listening for new blocks...");
    
    while let Some(header) = stream.next().await {
        let block_number = header.number;

        println!("\n--- [ New Block Received: {} ] ---", block_number);

        let opportunities = arbitrage_engine.find_opportunities(Some(block_number)).await;

        if opportunities.is_empty() {
            println!("No profitable opportunities found in this block.");
        } else {
            println!("[!] Found {} profitable opportunities!", opportunities.len());
            if let Some(top_opp) = opportunities.first() {
                let profit_eth = top_opp.gross_profit.as_limbs()[0] as f64 / 1e18;
                let input_eth = top_opp.optimal_input.as_limbs()[0] as f64 / 1e18;
                println!("    => Top Opp: Profit {:.6} ETH from {:.4} ETH input", profit_eth, input_eth);
            }
        }

        if block_number % 10 == 0 {
            println!("\nChecking for new pools since block {}...", last_seen_block);
            let (v2_discoveries, v3_discoveries, curve_discoveries) = tokio::join!(
                v2_pool_manager.discover_pools_in_range(block_number),
                v3_pool_manager.discover_pools_in_range(block_number),
                curve_pool_manager.discover_pools_in_range(block_number)
            );

            let new_pools_found = v2_discoveries.is_ok_and(|p| !p.is_empty())
                || v3_discoveries.is_ok_and(|p| !p.is_empty())
                || curve_discoveries.is_ok_and(|p| !p.is_empty());

            if new_pools_found {
                println!("New pools found! Rebuilding arbitrage paths...");
                let new_paths_2_pool = find_two_pool_cycles(&v2_pool_manager, &v3_pool_manager, &curve_pool_manager);
                let new_paths_3_pool = find_three_pool_cycles(&v2_pool_manager, &v3_pool_manager, &curve_pool_manager, &token_manager).await;
                let mut new_paths = new_paths_2_pool;
                new_paths.extend(new_paths_3_pool);

                arbitrage_cache.paths.write().await.clear();
                for path in new_paths {
                    arbitrage_cache.add_path(path).await;
                }
                println!("Updated to {} potential paths.", arbitrage_cache.paths.read().await.len());
            } else {
                println!("No new pools found.");
            }
            last_seen_block = block_number;
        }
    }
    Ok(())
}