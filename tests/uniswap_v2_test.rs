use alloy_primitives::{Address, B256, U256, address, b256};
use alloy_provider::{Provider, ProviderBuilder};
use arbrs::core::token::TokenLike;
use arbrs::dex::DexVariant;
use arbrs::manager::pool_manager::PoolManager;
use arbrs::manager::token_manager::TokenManager;
use arbrs::ArbRsError;
use arbrs::pool::LiquidityPool;
use arbrs::pool::strategy::{PancakeV2Logic, StandardV2Logic, V2CalculationStrategy};
use arbrs::pool::uniswap_v2::{UniswapV2Pool, UniswapV2PoolState};
use std::str::FromStr;
use std::sync::Arc;
use url::Url;

const WETH_ADDRESS: Address = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
const WBTC_ADDRESS: Address = address!("2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599");
const USDC_ADDRESS: Address = address!("A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");

const WBTC_WETH_POOL_ADDRESS: Address = address!("Bb2b8038a1640196FbE3e38816F3e67Cba72D940");
const SUSHISWAP_WETH_USDC_POOL: Address = address!("0x397FF1542f962076d0BFE58ea045ffa2d3473aee");

const V2_FACTORY_ADDRESS: Address = address!("5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f");
const V2_INIT_HASH: B256 =
    b256!("96e8ac4277198ff8b6f785478aa9a39f403cb768dd02cbee326c3e7da348845f");

const FORK_RPC_URL: &str = "http://127.0.0.1:8545";
type DynProvider = dyn Provider + Send + Sync;

/// Generic setup function to create a V2 pool with a specific strategy, returning a concrete type.
async fn setup_concrete_v2_pool<S: V2CalculationStrategy + Clone + 'static>(
    strategy: S,
    pool_address: Address,
    token_a_address: Address,
    token_b_address: Address,
) -> (
    Arc<TokenManager<DynProvider>>,
    Arc<UniswapV2Pool<DynProvider, S>>,
) {
    let url = Url::parse(FORK_RPC_URL).expect("Failed to parse RPC URL");
    let provider = ProviderBuilder::new().connect_http(url);
    let provider_arc: Arc<DynProvider> = Arc::new(provider);
    let manager = Arc::new(TokenManager::new(provider_arc.clone(), 1));

    let token_a = manager.get_token(token_a_address).await.unwrap();
    let token_b = manager.get_token(token_b_address).await.unwrap();
    let (token0, token1) = if token_a.address() < token_b.address() {
        (token_a, token_b)
    } else {
        (token_b, token_a)
    };

    let pool = Arc::new(UniswapV2Pool::new(
        pool_address,
        token0,
        token1,
        provider_arc,
        strategy,
    ));

    pool.update_state().await.unwrap();
    (manager, pool)
}

async fn setup_standard_v2_pool() -> (
    Arc<TokenManager<DynProvider>>,
    Arc<dyn LiquidityPool<DynProvider>>,
) {
    let (manager, pool) = setup_concrete_v2_pool(
        StandardV2Logic,
        WBTC_WETH_POOL_ADDRESS,
        WBTC_ADDRESS,
        WETH_ADDRESS,
    )
    .await;
    (manager, pool as Arc<dyn LiquidityPool<DynProvider>>)
}

async fn setup_pool_manager() -> PoolManager<DynProvider> {
    let url = Url::parse(FORK_RPC_URL).expect("Failed to parse RPC URL");
    let provider = ProviderBuilder::new().connect_http(url);
    let provider_arc: Arc<DynProvider> = Arc::new(provider);
    let token_manager = Arc::new(TokenManager::new(provider_arc.clone(), 1));
    PoolManager::new(token_manager, provider_arc)
}

#[tokio::test]
async fn test_v2_calculate_tokens_out() {
    let (manager, pool) = setup_standard_v2_pool().await;
    let wbtc = manager.get_token(WBTC_ADDRESS).await.unwrap();
    let amount_in = U256::from(8_000_000_000_u64);
    let expected_amount_out = U256::from_str("847228560678214929944").unwrap();

    let amount_out = pool.calculate_tokens_out(&wbtc, amount_in).await.unwrap();

    assert_eq!(amount_out, expected_amount_out);
}

#[tokio::test]
async fn test_v2_calculate_tokens_in() {
    let (manager, pool) = setup_standard_v2_pool().await;
    let weth = manager.get_token(WETH_ADDRESS).await.unwrap();
    let amount_out = U256::from_str("1200000000000000000000").unwrap();
    let expected_amount_in = U256::from(14_245_938_804_u64);

    let amount_in = pool
        .calculate_tokens_in_from_tokens_out(&weth, amount_out)
        .await
        .unwrap();

    assert_eq!(amount_in, expected_amount_in);
}

#[tokio::test]
async fn test_v2_input_validation() {
    let (manager, pool) = setup_standard_v2_pool().await;
    let wbtc = manager.get_token(WBTC_ADDRESS).await.unwrap();
    let dai = manager
        .get_token(address!("6B175474E89094C44Da98b954EedeAC495271d0F"))
        .await
        .unwrap();

    let result_zero_in = pool.calculate_tokens_out(&wbtc, U256::ZERO).await;
    assert!(matches!(
        result_zero_in,
        Err(ArbRsError::CalculationError(_))
    ));

    let result_zero_out = pool
        .calculate_tokens_in_from_tokens_out(&wbtc, U256::ZERO)
        .await;
    assert!(matches!(
        result_zero_out,
        Err(ArbRsError::CalculationError(_))
    ));

    let result_wrong_token = pool.calculate_tokens_out(&dai, U256::from(1000)).await;
    assert!(matches!(
        result_wrong_token,
        Err(ArbRsError::CalculationError(_))
    ));
}

#[tokio::test]
async fn test_v2_price_calculation() {
    let (_manager, pool) = setup_standard_v2_pool().await;
    let (wbtc, weth) = pool.tokens();

    let nominal_price = pool.nominal_price().await.unwrap();
    let absolute_price = pool.absolute_price().await.unwrap();

    let expected_nominal =
        absolute_price * 10_f64.powi(wbtc.decimals() as i32 - weth.decimals() as i32);
    assert!(
        (nominal_price - expected_nominal).abs() < 1e-9,
        "Nominal price calculation mismatch"
    );
}

#[tokio::test]
async fn test_v2_insufficient_liquidity_swap() {
    let (manager, pool) = setup_standard_v2_pool().await;
    let weth = manager.get_token(WETH_ADDRESS).await.unwrap();

    let concrete_pool = pool
        .as_any()
        .downcast_ref::<UniswapV2Pool<DynProvider, StandardV2Logic>>()
        .unwrap();

    let current_state = concrete_pool.get_cached_reserves().await;
    let (token0, _) = concrete_pool.tokens();

    let weth_reserve = if weth.address() == token0.address() {
        current_state.reserve0
    } else {
        current_state.reserve1
    };

    let result = pool
        .calculate_tokens_in_from_tokens_out(&weth, weth_reserve)
        .await;

    assert!(
        matches!(result, Err(ArbRsError::CalculationError(ref msg)) if msg.contains("Insufficient liquidity")),
        "Test failed: expected 'Insufficient liquidity' error, got {:?}",
        result
    );
}

#[test]
fn test_v2_pool_address_generator() {
    let token_a = WETH_ADDRESS;
    let token_b = WBTC_ADDRESS;

    let calculated_address = UniswapV2Pool::<DynProvider, StandardV2Logic>::calculate_pool_address(
        token_a,
        token_b,
        V2_FACTORY_ADDRESS,
        V2_INIT_HASH,
    );

    assert_eq!(calculated_address, WBTC_WETH_POOL_ADDRESS);
}

#[tokio::test]
async fn test_v2_state_override_calculation() {
    let (manager, pool) = setup_concrete_v2_pool(
        StandardV2Logic,
        WBTC_WETH_POOL_ADDRESS,
        WBTC_ADDRESS,
        WETH_ADDRESS,
    )
    .await;
    let wbtc = manager.get_token(WBTC_ADDRESS).await.unwrap();
    let weth = manager.get_token(WETH_ADDRESS).await.unwrap();

    let amount_in = U256::from(100_000_000);

    let override_reserves = UniswapV2PoolState {
        reserve0: U256::from(2000) * U256::from(10).pow(U256::from(wbtc.decimals())),
        reserve1: U256::from(30000) * U256::from(10).pow(U256::from(weth.decimals())),
    };

    let live_amount_out = pool.calculate_tokens_out(&wbtc, amount_in).await.unwrap();

    let override_amount_out = pool
        .calculate_tokens_out_with_override(&wbtc, amount_in, &override_reserves)
        .unwrap();

    assert_ne!(live_amount_out, override_amount_out);

    let expected_override_out = U256::from_str("14947548646999470763").unwrap();
    assert_eq!(
        override_amount_out, expected_override_out,
        "Override calculation mismatch"
    );
}

#[tokio::test]
async fn test_v2_custom_fee_strategy() {
    let url = Url::parse(FORK_RPC_URL).expect("Failed to parse RPC URL");
    let provider_arc: Arc<DynProvider> = Arc::new(ProviderBuilder::new().connect_http(url));
    let manager = Arc::new(TokenManager::new(provider_arc.clone(), 1));
    let token0 = manager.get_token(WBTC_ADDRESS).await.unwrap();
    let token1 = manager.get_token(WETH_ADDRESS).await.unwrap();

    let pool_default_fee = UniswapV2Pool::new(
        WBTC_WETH_POOL_ADDRESS,
        token0.clone(),
        token1.clone(),
        provider_arc.clone(),
        StandardV2Logic {},
    );

    let pool_custom_fee = UniswapV2Pool::new(
        WBTC_WETH_POOL_ADDRESS,
        token0.clone(),
        token1.clone(),
        provider_arc.clone(),
        PancakeV2Logic {},
    );

    pool_default_fee.update_state().await.unwrap();
    let live_state = pool_default_fee.get_cached_reserves().await;
    let amount_in = U256::from(10_u64.pow(token0.decimals() as u32));

    let out_default_fee = pool_default_fee
        .calculate_tokens_out_with_override(&token0, amount_in, &live_state)
        .unwrap();
    let out_custom_fee = pool_custom_fee
        .calculate_tokens_out_with_override(&token0, amount_in, &live_state)
        .unwrap();

    assert!(
        out_custom_fee > out_default_fee,
        "Lower fee strategy did not result in a higher output amount. Custom (25bps): {}, Default (30bps): {}",
        out_custom_fee,
        out_default_fee
    );
}

#[tokio::test]
async fn test_pool_manager_fee_assignment() {
    let pool_manager = setup_pool_manager().await;

    println!("Building Uniswap V2 pool via manager...");
    let build_result_uni = pool_manager
        .build_v2_pool(
            WBTC_WETH_POOL_ADDRESS,
            WBTC_ADDRESS,
            WETH_ADDRESS,
            DexVariant::UniswapV2,
        )
        .await;
    assert!(
        build_result_uni.is_ok(),
        "Failed to build Uniswap pool: {:?}",
        build_result_uni.err()
    );
    let uni_pool = build_result_uni.unwrap();

    let uni_pool_concrete = uni_pool
        .as_any()
        .downcast_ref::<UniswapV2Pool<DynProvider, StandardV2Logic>>()
        .expect("Failed to downcast Uniswap pool to V2 with StandardV2Logic");
    assert_eq!(uni_pool_concrete.strategy().get_fee_bps(), 30);

    println!("Building Sushiswap pool via manager...");
    let build_result_sushi = pool_manager
        .build_v2_pool(
            SUSHISWAP_WETH_USDC_POOL,
            WETH_ADDRESS,
            USDC_ADDRESS,
            DexVariant::SushiSwap,
        )
        .await;
    assert!(
        build_result_sushi.is_ok(),
        "Failed to build Sushiswap pool: {:?}",
        build_result_sushi.err()
    );
    let sushi_pool = build_result_sushi.unwrap();

    let sushi_pool_concrete = sushi_pool
        .as_any()
        .downcast_ref::<UniswapV2Pool<DynProvider, StandardV2Logic>>()
        .expect("Failed to downcast Sushiswap pool to V2 with StandardV2Logic");

    assert_eq!(sushi_pool_concrete.strategy().get_fee_bps(), 30);
}
