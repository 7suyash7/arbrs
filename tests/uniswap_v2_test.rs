use alloy_primitives::{address, Address, B256, U256, b256};
use alloy_provider::{Provider, ProviderBuilder};
use arbrs::{
    db::DbManager,
    dex::DexVariant,
    manager::{
        token_manager::TokenManager, uniswap_v2_pool_manager::UniswapV2PoolManager,
    },
    pool::{
        strategy::StandardV2Logic,
        uniswap_v2::{UniswapV2Pool, UniswapV2PoolState},
        LiquidityPool
    },
    TokenLike,
};
use std::{str::FromStr, sync::Arc};

const WETH_ADDRESS: Address = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
const WBTC_ADDRESS: Address = address!("2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599");
const USDC_ADDRESS: Address = address!("A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");
const WBTC_WETH_POOL_ADDRESS: Address = address!("Bb2b8038a1640196FbE3e38816F3e67Cba72D940");
const SUSHISWAP_WETH_USDC_POOL: Address = address!("0x397FF1542f962076d0BFE58ea045ffa2d3473aee");
const V2_FACTORY_ADDRESS: Address = address!("5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f");
const V2_INIT_HASH: B256 = b256!("96e8ac4277198ff8b6f785478aa9a39f403cb768dd02cbee326c3e7da348845f");
const FORK_RPC_URL: &str = "http://127.0.0.1:8545";
const DB_URL: &str = "sqlite::memory:";
type DynProvider = dyn Provider + Send + Sync;


async fn setup() -> (Arc<DynProvider>, Arc<DbManager>, Arc<TokenManager<DynProvider>>) {
    let provider = ProviderBuilder::new().connect_http(FORK_RPC_URL.parse().unwrap());
    let provider_arc: Arc<DynProvider> = Arc::new(provider);
    let db_manager = Arc::new(DbManager::new(DB_URL).await.unwrap());
    let token_manager = Arc::new(TokenManager::new(provider_arc.clone(), 1, db_manager.clone()));
    (provider_arc, db_manager, token_manager)
}

#[tokio::test]
async fn test_v2_calculate_tokens_out() {
    let (provider, _, token_manager) = setup().await;
    let weth = token_manager.get_token(WETH_ADDRESS).await.unwrap();
    let wbtc = token_manager.get_token(WBTC_ADDRESS).await.unwrap();
    let pool = UniswapV2Pool::new(WBTC_WETH_POOL_ADDRESS, wbtc.clone(), weth.clone(), provider, StandardV2Logic);

    let snapshot = pool.get_snapshot(Some(19000000)).await.unwrap();
    
    let amount_in = U256::from(10_000_000);
    let expected_amount_out = U256::from_str("1665004674849186750").unwrap();
    let amount_out = pool.calculate_tokens_out(&wbtc, &weth, amount_in, &snapshot).unwrap();

    assert_eq!(amount_out, expected_amount_out);
}

#[tokio::test]
async fn test_v2_calculate_tokens_in() {
    let (provider, _, token_manager) = setup().await;
    let weth = token_manager.get_token(WETH_ADDRESS).await.unwrap();
    let wbtc = token_manager.get_token(WBTC_ADDRESS).await.unwrap();
    let pool = UniswapV2Pool::new(WBTC_WETH_POOL_ADDRESS, wbtc.clone(), weth.clone(), provider, StandardV2Logic);

    let snapshot = pool.get_snapshot(Some(19000000)).await.unwrap();

    let amount_out = U256::from_str("1000000000000000000").unwrap();
     let expected_amount_in = U256::from(6003852);
    let amount_in = pool.calculate_tokens_in(&wbtc, &weth, amount_out, &snapshot).unwrap();
    
    assert_eq!(amount_in, expected_amount_in);
}

#[test]
fn test_v2_pool_address_generator() {
    let token_a = WETH_ADDRESS;
    let token_b = WBTC_ADDRESS;
    let calculated_address = UniswapV2Pool::<DynProvider, StandardV2Logic>::calculate_pool_address(
        token_a, token_b, V2_FACTORY_ADDRESS, V2_INIT_HASH,
    );
    assert_eq!(calculated_address, WBTC_WETH_POOL_ADDRESS);
}

#[tokio::test]
async fn test_v2_state_override_calculation() {
    let (provider, _, token_manager) = setup().await;
    let weth = token_manager.get_token(WETH_ADDRESS).await.unwrap();
    let wbtc = token_manager.get_token(WBTC_ADDRESS).await.unwrap();
    let pool = UniswapV2Pool::new(WBTC_WETH_POOL_ADDRESS, wbtc.clone(), weth.clone(), provider, StandardV2Logic);

    let live_snapshot = pool.get_snapshot(None).await.unwrap();
    let amount_in = U256::from(100_000_000);
    let live_amount_out = pool.calculate_tokens_out(&wbtc, &weth, amount_in, &live_snapshot).unwrap();

    let override_state = UniswapV2PoolState {
        reserve0: U256::from(2000) * U256::from(10).pow(U256::from(wbtc.decimals())),
        reserve1: U256::from(30000) * U256::from(10).pow(U256::from(weth.decimals())),
        block_number: 0,
    };

    let override_amount_out = pool
        .calculate_tokens_out_with_override(&wbtc, &weth, amount_in, &override_state)
        .unwrap();

    assert_ne!(live_amount_out, override_amount_out);
    let expected_override_out = U256::from_str("14947548646999470763").unwrap();
    assert_eq!(override_amount_out, expected_override_out, "Override calculation mismatch");
}

#[tokio::test]
async fn test_pool_manager_creation() {
    let (_provider, _db_manager, token_manager) = setup().await;
    let pool_manager = UniswapV2PoolManager::new(
        token_manager, _provider, V2_FACTORY_ADDRESS, 0
    );

    let pool = pool_manager.build_v2_pool(
        SUSHISWAP_WETH_USDC_POOL, WETH_ADDRESS, USDC_ADDRESS, DexVariant::SushiSwap
    ).await.unwrap();

    assert_eq!(pool.address(), SUSHISWAP_WETH_USDC_POOL);
    assert_eq!(pool_manager.get_all_pools().len(), 1);
}