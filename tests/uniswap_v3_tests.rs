use alloy::sol;
use alloy_primitives::{Address, I256, U160, U256, address, aliases::U24};
use alloy_provider::{Provider, ProviderBuilder};
use alloy_rpc_types::TransactionRequest;
use alloy_sol_types::SolCall;
use arbrs::pool::uniswap_v3_snapshot::UniswapV3LiquiditySnapshot;
use arbrs::TokenLike;
use arbrs::core::token::Token;
use arbrs::pool::LiquidityPool;
use arbrs::pool::uniswap_v3::UniswapV3Pool;
use serde_json::Value;
use std::fs;
use arbrs::pool::uniswap_v3::{TickInfo, UniswapV3PoolState};
use arbrs::{
    TokenManager,
    core::token::Erc20Data,
    manager::uniswap_v3_pool_manager::UniswapV3PoolManager,
    math::v3::{
        sqrt_price_math::{self, MAX_U160},
        swap_math::{self},
        tick::Tick,
        utils::sqrt,
    },
};
use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::Arc;
use url::Url;

const FORK_RPC_URL: &str = "http://127.0.0.1:8545";
const WBTC_WETH_V3_POOL_ADDRESS: Address = address!("CBCdF9626bC03E24f779434178A73a0B4bad62eD");
const WETH_ADDRESS: Address = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
const WBTC_ADDRESS: Address = address!("2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599");
const V3_FACTORY_ADDRESS: Address = address!("1F98431c8aD98523631AE4a59f267346ea31F984");
const EMPTY_SNAPSHOT_BLOCK: u64 = 12_369_620;

sol! {
    interface IQuoter {
        function quoteExactInputSingle(
            address tokenIn,
            address tokenOut,
            uint24 fee,
            uint256 amountIn,
            uint160 sqrtPriceLimitX96
        ) external returns (uint256 amountOut);
    }
}

type DynProvider = dyn Provider + Send + Sync;

fn setup_dummy_v3_pool_with_provider() -> UniswapV3Pool<DynProvider> {
    let url = Url::parse(FORK_RPC_URL).expect("Failed to parse RPC URL");
    let provider = ProviderBuilder::new().connect_http(url);
    let provider_arc: Arc<DynProvider> = Arc::new(provider);

    let dummy_address = address!("0000000000000000000000000000000000000001");
    // Dummy tokens for type correctness, decimals are important for nominal_price test
    let token0 = Arc::new(Token::Erc20(Arc::new(Erc20Data::new(
        address!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        "TKA".to_string(),
        "Token A".to_string(),
        18,
        provider_arc.clone(),
    ))));
    let token1 = Arc::new(Token::Erc20(Arc::new(Erc20Data::new(
        address!("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
        "TKB".to_string(),
        "Token B".to_string(),
        6,
        provider_arc.clone(),
    ))));

    UniswapV3Pool::new(dummy_address, token0, token1, 3000, 60, provider_arc, None)
}

async fn setup_v3_pool_manager() -> (
    Arc<TokenManager<DynProvider>>,
    UniswapV3PoolManager<DynProvider>,
) {
    let url = Url::parse(FORK_RPC_URL).expect("Failed to parse RPC URL");
    let provider = ProviderBuilder::new().connect_http(url);
    let provider_arc: Arc<DynProvider> = Arc::new(provider);
    let token_manager = Arc::new(TokenManager::new(provider_arc.clone(), 1));
    let pool_manager = UniswapV3PoolManager::new(
        token_manager.clone(), 
        provider_arc, 
        1,
        0,
        V3_FACTORY_ADDRESS,
    );
    (token_manager, pool_manager)

}

fn e18(n: u64) -> U256 {
    U256::from(n) * U256::from(10).pow(U256::from(18))
}

fn encode_price_sqrt(reserve1: u128, reserve0: u128) -> U256 {
    let r1 = U256::from(reserve1);
    let r0 = U256::from(reserve0);
    sqrt(r1 * (U256::from(1) << 192) / r0)
}

#[test]
fn test_get_amount_delta_tests() {
    let amount0 = sqrt_price_math::get_amount0_delta(
        encode_price_sqrt(1, 1),
        encode_price_sqrt(121, 100),
        e18(1).to::<u128>(),
        true,
    )
    .unwrap();
    assert_eq!(amount0, U256::from_str("90909090909090910").unwrap());

    let amount0_rounded_down = sqrt_price_math::get_amount0_delta(
        encode_price_sqrt(1, 1),
        encode_price_sqrt(121, 100),
        e18(1).to::<u128>(),
        false,
    )
    .unwrap();
    assert_eq!(amount0_rounded_down, amount0 - U256::from(1));

    let amount1 = sqrt_price_math::get_amount1_delta(
        encode_price_sqrt(1, 1),
        encode_price_sqrt(121, 100),
        e18(1).to::<u128>(),
        true,
    )
    .unwrap();
    assert_eq!(amount1, U256::from_str("100000000000000000").unwrap());

    let amount1_rounded_down = sqrt_price_math::get_amount1_delta(
        encode_price_sqrt(1, 1),
        encode_price_sqrt(121, 100),
        e18(1).to::<u128>(),
        false,
    )
    .unwrap();
    assert_eq!(amount1_rounded_down, amount1 - U256::from(1));
}

#[test]
fn test_get_next_sqrt_price_from_input_tests() {
    let price = encode_price_sqrt(1, 1);
    let liquidity = e18(1).to::<u128>();

    assert!(
        sqrt_price_math::get_next_sqrt_price_from_input(price, 0, U256::from(1), true).is_err()
    );
    assert_eq!(
        sqrt_price_math::get_next_sqrt_price_from_input(price, liquidity, U256::ZERO, true)
            .unwrap(),
        price
    );
    assert_eq!(
        sqrt_price_math::get_next_sqrt_price_from_input(price, liquidity, U256::ZERO, false)
            .unwrap(),
        price
    );
    assert!(
        sqrt_price_math::get_next_sqrt_price_from_input(MAX_U160, 1024, U256::from(1024), false)
            .is_err()
    );

    let sqrt_q_false = sqrt_price_math::get_next_sqrt_price_from_input(
        price,
        liquidity,
        e18(1) / U256::from(10),
        false,
    )
    .unwrap();
    assert_eq!(
        sqrt_q_false,
        U256::from_str("87150978765690771352898345369").unwrap()
    );

    let sqrt_q_true = sqrt_price_math::get_next_sqrt_price_from_input(
        price,
        liquidity,
        e18(1) / U256::from(10),
        true,
    )
    .unwrap();
    assert_eq!(
        sqrt_q_true,
        U256::from_str("72025602285694852357767227579").unwrap()
    );
}

#[test]
fn test_all_swap_scenarios() {
    // exact amount IN that gets capped at price target
    let price = encode_price_sqrt(1, 1);
    let price_target = encode_price_sqrt(101, 100);
    let liquidity = e18(2).to::<u128>();
    let amount = I256::from_raw(e18(1));
    let fee = 600;

    let result = swap_math::compute_swap_step(price, price_target, liquidity, amount, fee).unwrap();
    let pre_fee_amount_in = result.amount_in - result.fee_amount;

    assert_eq!(
        pre_fee_amount_in,
        U256::from_str("9975124224178055").unwrap()
    );
    assert_eq!(result.fee_amount, U256::from_str("5988667735148").unwrap());
    assert_eq!(
        result.amount_out,
        U256::from_str("9925619580021728").unwrap()
    );
    assert!(result.amount_in < amount.into_raw());
    assert_eq!(result.sqrt_ratio_next_x96, price_target);

    // exact amount OUT that gets capped at price target
    let result_out =
        swap_math::compute_swap_step(price, price_target, liquidity, -amount, fee).unwrap();
    let pre_fee_amount_in_2 = result_out.amount_in - result_out.fee_amount;
    assert_eq!(
        pre_fee_amount_in_2,
        U256::from_str("9975124224178055").unwrap()
    );
    assert_eq!(
        result_out.fee_amount,
        U256::from_str("5988667735148").unwrap()
    );
    assert_eq!(
        result_out.amount_out,
        U256::from_str("9925619580021728").unwrap()
    );
    assert!(result_out.amount_out < (-amount).into_raw());
    assert_eq!(result_out.sqrt_ratio_next_x96, price_target);

    // exact amount IN that is fully spent
    let price_target_full = U256::from_str("2505413383311432194396931511005").unwrap();
    let result_full =
        swap_math::compute_swap_step(price, price_target_full, liquidity, amount, fee).unwrap();
    let pre_fee_amount_in_3 = result_full.amount_in - result_full.fee_amount;
    assert_eq!(
        pre_fee_amount_in_3,
        U256::from_str("999400000000000000").unwrap()
    );
    assert_eq!(
        result_full.fee_amount,
        U256::from_str("600000000000000").unwrap()
    );
    assert_eq!(
        result_full.amount_out,
        U256::from_str("666399946655997866").unwrap()
    );
    assert_eq!(result_full.amount_in, amount.into_raw());

    // exact amount OUT that is fully received
    let price_target_full_out = encode_price_sqrt(10000, 100);
    let amount_out_target = I256::from_raw(e18(1));
    let result_full_out = swap_math::compute_swap_step(
        price,
        price_target_full_out,
        liquidity,
        -amount_out_target,
        fee,
    )
    .unwrap();
    let pre_fee_amount_in_4 = result_full_out.amount_in - result_full_out.fee_amount;
    assert_eq!(
        pre_fee_amount_in_4,
        U256::from_str("2000000000000000000").unwrap()
    );
    assert_eq!(
        result_full_out.fee_amount,
        U256::from_str("1200720432259356").unwrap()
    );
    assert_eq!(result_full_out.amount_out, amount_out_target.into_raw());

    // amount OUT is capped at the desired amount out
    let result_cap_out = swap_math::compute_swap_step(
        U256::from_str("417332158212080721273783715441582").unwrap(),
        U256::from_str("1452870262520218020823638996").unwrap(),
        159344665391607089467575320103,
        I256::from_str("-1").unwrap(),
        1,
    )
    .unwrap();
    assert_eq!(result_cap_out.amount_in, U256::from(2)); // pre-fee 1 + fee 1
    assert_eq!(result_cap_out.amount_out, U256::from(1));
    assert_eq!(result_cap_out.fee_amount, U256::from(1));
    assert_eq!(
        result_cap_out.sqrt_ratio_next_x96,
        U256::from_str("417332158212080721273783715441581").unwrap()
    );

    // entire input amount taken as fee
    let result_fee = swap_math::compute_swap_step(
        U256::from(2413),
        U256::from_str("79887613182836312").unwrap(),
        1985041575832132834610021537970,
        I256::from_str("10").unwrap(),
        1872,
    )
    .unwrap();
    assert_eq!(result_fee.amount_in, U256::from(10));
    assert_eq!(result_fee.fee_amount, U256::from(10));
    assert_eq!(result_fee.amount_out, U256::ZERO);
    assert_eq!(result_fee.sqrt_ratio_next_x96, U256::from(2413));
}

#[test]
fn test_tick_info_equality() {
    let tick_info1 = TickInfo {
        liquidity_gross: 80064092962998,
        liquidity_net: 80064092962998,
    };

    let tick_info2 = TickInfo {
        liquidity_gross: 80064092962998,
        liquidity_net: 80064092962998,
    };

    assert_eq!(tick_info1, tick_info2);
}

#[test]
fn test_tick_struct_defaults() {
    let default_tick = Tick::default();

    assert_eq!(default_tick.liquidity_gross, 0);
    assert_eq!(default_tick.liquidity_net, 0);
    assert!(!default_tick.initialized);
}

#[test]
fn test_pool_state_defaults() {
    let default_state = UniswapV3PoolState::default();
    assert_eq!(default_state.liquidity, 0);
    assert_eq!(default_state.sqrt_price_x96, U256::ZERO);
    assert_eq!(default_state.tick, 0);
    assert_eq!(default_state.block_number, 0);
    assert_eq!(default_state.tick_bitmap, BTreeMap::new());
    assert_eq!(default_state.tick_data, BTreeMap::new());
}

#[tokio::test]
async fn test_v3_exchange_rate_from_sqrt_price() {
    let pool = setup_dummy_v3_pool_with_provider();
    let weth_usdc_sqrt_price_x96 = U256::from_str("2018382873588440326581633304624437").unwrap();

    {
        let mut state = pool.state.write().await;
        state.sqrt_price_x96 = weth_usdc_sqrt_price_x96;
    }

    let price = pool.absolute_price().await.unwrap();

    let sqrt_price_f64 = weth_usdc_sqrt_price_x96.to_string().parse::<f64>().unwrap();
    let q96_f64 = 2_f64.powi(96);
    let expected_price = (sqrt_price_f64 / q96_f64).powi(2);

    assert!(
        (price - expected_price).abs() < 1e-9,
        "Price calculation mismatch. Got {}, expected {}",
        price,
        expected_price
    );
}

#[tokio::test]
async fn test_pool_creation() {
    let (_token_manager, pool_manager) = setup_v3_pool_manager().await;

    let pool_result = pool_manager
        .build_pool(
            WBTC_WETH_V3_POOL_ADDRESS,
            WBTC_ADDRESS,
            WETH_ADDRESS,
            3000,
            60,
        )
        .await;
    assert!(pool_result.is_ok());
}

#[tokio::test]
async fn test_pool_creation_with_liquidity_map() {
    let (_token_manager, provider) = {
        let url = Url::parse(FORK_RPC_URL).expect("Failed to parse RPC URL");
        let provider = ProviderBuilder::new().connect_http(url);
        let provider_arc: Arc<DynProvider> = Arc::new(provider);
        let token_manager = Arc::new(TokenManager::new(provider_arc.clone(), 1));
        (token_manager, provider_arc)
    };

    let token0 = TokenManager::new(provider.clone(), 1)
        .get_token(WBTC_ADDRESS)
        .await
        .unwrap();
    let token1 = TokenManager::new(provider.clone(), 1)
        .get_token(WETH_ADDRESS)
        .await
        .unwrap();

    let pool = UniswapV3Pool::new(
        WBTC_WETH_V3_POOL_ADDRESS,
        token0,
        token1,
        3000,
        60,
        provider,
        Some(Default::default()),
    );

    let state = pool.state.read().await;
    assert!(state.tick_bitmap.is_empty());
    assert!(state.tick_data.is_empty());
}

#[tokio::test]
async fn test_price_is_inverse_of_exchange_rate() {
    let (_token_manager, pool_manager) = setup_v3_pool_manager().await;
    let pool = pool_manager
        .build_pool(
            WBTC_WETH_V3_POOL_ADDRESS,
            WBTC_ADDRESS,
            WETH_ADDRESS,
            3000,
            60,
        )
        .await
        .unwrap();
    pool.update_state().await.unwrap();

    let price = pool.absolute_price().await.unwrap();
    let exchange_rate = pool.absolute_exchange_rate().await.unwrap();

    assert!(
        (price - (1.0 / exchange_rate)).abs() < 1e-9,
        "Price ({}) should be the inverse of the exchange rate ({})",
        price,
        exchange_rate
    );
    assert!(
        (exchange_rate - (1.0 / price)).abs() < 1e-9,
        "Exchange rate ({}) should be the inverse of the price ({})",
        exchange_rate,
        price
    );
}

#[tokio::test]
async fn test_v3_swap_calculations_match_quoter() {
    let url = Url::parse(FORK_RPC_URL).expect("Failed to parse RPC URL");
    let provider = ProviderBuilder::new().connect_http(url);
    let provider_arc: Arc<DynProvider> = Arc::new(provider);

    let (_token_manager, pool_manager) = setup_v3_pool_manager().await;
    let pool = pool_manager
        .build_pool(
            WBTC_WETH_V3_POOL_ADDRESS,
            WBTC_ADDRESS,
            WETH_ADDRESS,
            3000,
            60,
        )
        .await
        .unwrap();
    pool.update_state().await.unwrap();

    let (wbtc, weth) = pool.tokens();
    let quoter_address = address!("b27308f9F90D607463bb33eA1BeBb41C27CE5AB6");

    let amount_in_wbtc = U256::from(100_000_000);

    let local_amount_out_weth = pool
        .calculate_tokens_out(&wbtc, amount_in_wbtc)
        .await
        .unwrap();

    let quoter_call = IQuoter::quoteExactInputSingleCall {
        tokenIn: wbtc.address(),
        tokenOut: weth.address(),
        fee: U24::from(3000),
        amountIn: amount_in_wbtc,
        sqrtPriceLimitX96: U160::ZERO,
    };
    let request = TransactionRequest::default()
        .to(quoter_address)
        .input(quoter_call.abi_encode().into());
    let result_bytes = provider_arc.call(request).await.unwrap();
    let onchain_amount_out_weth =
        IQuoter::quoteExactInputSingleCall::abi_decode_returns(&result_bytes).unwrap();

    assert_eq!(local_amount_out_weth, onchain_amount_out_weth);

    let amount_in_weth = U256::from(10) * U256::from(10).pow(U256::from(18)); // 10 WETH

    let local_amount_out_wbtc = pool
        .calculate_tokens_out(&weth, amount_in_weth)
        .await
        .unwrap();

    let quoter_call_2 = IQuoter::quoteExactInputSingleCall {
        tokenIn: weth.address(),
        tokenOut: wbtc.address(),
        fee: U24::from(3000),
        amountIn: amount_in_weth,
        sqrtPriceLimitX96: U160::ZERO,
    };
    let request_2 = TransactionRequest::default()
        .to(quoter_address)
        .input(quoter_call_2.abi_encode().into());
    let result_bytes_2 = provider_arc.call(request_2).await.unwrap();
    let onchain_amount_out_wbtc =
        IQuoter::quoteExactInputSingleCall::abi_decode_returns(&result_bytes_2).unwrap();

    assert_eq!(local_amount_out_wbtc, onchain_amount_out_wbtc);
}

#[tokio::test]
async fn test_v3_simulations() {
    let (_token_manager, pool_manager) = setup_v3_pool_manager().await;
    let pool_arc = pool_manager.build_pool(
        WBTC_WETH_V3_POOL_ADDRESS,
        WBTC_ADDRESS,
        WETH_ADDRESS,
        3000,
        60
    ).await.unwrap();

    let pool = pool_arc.as_any().downcast_ref::<UniswapV3Pool<DynProvider>>().unwrap();

    pool.update_state().await.unwrap();

    let (wbtc, weth) = pool.tokens();
    let initial_state = pool.state.read().await.clone();

    let amount_in_wbtc = U256::from(100_000_000);

    let sim_result = pool
        .simulate_exact_input_swap(&wbtc, amount_in_wbtc, None)
        .await
        .unwrap();

    assert_eq!(sim_result.initial_state, initial_state);

    assert_eq!(sim_result.amount0_delta, I256::from_raw(amount_in_wbtc));
    let expected_weth_out = pool.calculate_tokens_out(&wbtc, amount_in_wbtc).await.unwrap();
    assert_eq!(sim_result.amount1_delta, -I256::from_raw(expected_weth_out));

    let override_state = arbrs::pool::uniswap_v3::UniswapV3PoolState {
        liquidity: 1533143241938066251,
        sqrt_price_x96: U256::from_str("31881290961944305252140777263703426").unwrap(),
        tick: 258116,
        ..Default::default()
    };

    let amount_in_weth = U256::from(1) * U256::from(10).pow(U256::from(18));

    let override_sim_result = pool
        .simulate_exact_input_swap(&weth, amount_in_weth, Some(&override_state))
        .await
        .unwrap();

    let expected_final_state = arbrs::pool::uniswap_v3::UniswapV3PoolState {
        liquidity: 1533143241938066251,
        sqrt_price_x96: U256::from_str("31881342483860761583159860586051776").unwrap(),
        tick: 258116,
        ..Default::default()
    };

    assert_eq!(override_sim_result.amount0_delta, -I256::try_from(6157179).unwrap());
    assert_eq!(override_sim_result.amount1_delta, I256::from_raw(amount_in_weth));
    assert_eq!(override_sim_result.final_state.sqrt_price_x96, expected_final_state.sqrt_price_x96);
    assert_eq!(override_sim_result.final_state.tick, expected_final_state.tick);
    assert_eq!(override_sim_result.final_state.liquidity, expected_final_state.liquidity);
}

#[tokio::test]
async fn test_fetch_liquidity_events() {
    let url = Url::parse(FORK_RPC_URL).expect("Failed to parse RPC URL");
    let provider = ProviderBuilder::new().connect_http(url);
    let provider_arc: Arc<DynProvider> = Arc::new(provider);

    let mut snapshot = UniswapV3LiquiditySnapshot::new(provider_arc, 1, EMPTY_SNAPSHOT_BLOCK);

    let end_block = EMPTY_SNAPSHOT_BLOCK + 250;
    snapshot.fetch_new_events(end_block).await.unwrap();
    let wbtc_weth_pool = address!("CBCdF9626bC03E24f779434178A73a0B4bad62eD");
    let wbtc_weth_events = snapshot.liquidity_events.get(&wbtc_weth_pool).unwrap();

    assert_eq!(wbtc_weth_events.len(), 2);

    let event1 = &wbtc_weth_events[0];
    assert_eq!(event1.block_number, 12369821);
    assert_eq!(event1.liquidity, 34399999543676);
    assert_eq!(event1.tick_lower, 253320);
    assert_eq!(event1.tick_upper, 264600);

    let event2 = &wbtc_weth_events[1];
    assert_eq!(event2.block_number, 12369846);
    assert_eq!(event2.liquidity, 2154941425);
    assert_eq!(event2.tick_lower, 255540);
    assert_eq!(event2.tick_upper, 262440);
}

#[tokio::test]
async fn test_pool_manager_applies_snapshot_from_file() {
    let snapshot_path = "tests/test_snapshot.json";
    let data = fs::read_to_string(snapshot_path).expect("Unable to read snapshot file");
    let json_data: Value = serde_json::from_str(&data).expect("Unable to parse JSON");

    let pool_address_str = "0xCBCdF9626bC03E24f779434178A73a0B4bad62eD";
    let pool_data = &json_data[pool_address_str];

    let mut tick_bitmap = BTreeMap::new();
    for (word, bitmap_data) in pool_data["tick_bitmap"].as_object().unwrap() {
        tick_bitmap.insert(
            word.parse::<i16>().unwrap(),
            U256::from_str(bitmap_data["bitmap"].as_str().unwrap()).unwrap(),
        );
    }

    let mut tick_data = BTreeMap::new();
    for (tick, tick_info_data) in pool_data["tick_data"].as_object().unwrap() {
        tick_data.insert(
            tick.parse::<i32>().unwrap(),
            TickInfo {
                liquidity_gross: tick_info_data["liquidity_gross"].as_str().unwrap().parse().unwrap(),
                liquidity_net: tick_info_data["liquidity_net"].as_str().unwrap().parse().unwrap(),
            },
        );
    }

    let (_token_manager, provider) = {
        let url = Url::parse(FORK_RPC_URL).expect("Failed to parse RPC URL");
        let provider = ProviderBuilder::new().connect_http(url);
        let provider_arc: Arc<DynProvider> = Arc::new(provider);
        let token_manager = Arc::new(TokenManager::new(provider_arc.clone(), 1));
        (token_manager, provider_arc)
    };

    let token0 = TokenManager::new(provider.clone(), 1).get_token(WBTC_ADDRESS).await.unwrap();
    let token1 = TokenManager::new(provider.clone(), 1).get_token(WETH_ADDRESS).await.unwrap();

    let pool = UniswapV3Pool::new(
        Address::from_str(pool_address_str).unwrap(),
        token0,
        token1,
        3000,
        60,
        provider,
        Some(arbrs::pool::uniswap_v3_snapshot::LiquidityMap { tick_bitmap: tick_bitmap.clone(), tick_data: tick_data.clone() })
    );

    let state = pool.state.read().await;
    assert_eq!(state.tick_bitmap, tick_bitmap);
    assert_eq!(state.tick_data, tick_data);
}

#[tokio::test]
async fn test_v3_pool_discovery() {
    // 1. Setup provider to connect to your Anvil fork
    let url = Url::parse(FORK_RPC_URL).expect("Failed to parse RPC URL");
    let provider = ProviderBuilder::new().connect_http(url);
    let provider_arc: Arc<DynProvider> = Arc::new(provider);
    let token_manager = Arc::new(TokenManager::new(provider_arc.clone(), 1));

    let start_block = 13616453;
    let end_block = 13616454;

    let mut pool_manager = UniswapV3PoolManager::new(
        token_manager,
        provider_arc.clone(),
        1,
        start_block,
        V3_FACTORY_ADDRESS,
    );

    let new_pools = pool_manager.discover_pools_in_range(end_block).await.unwrap();

    assert!(!new_pools.is_empty(), "discover_pools should have found the USDC/WETH 0.01% pool");

    let usdc_weth_pool_address = address!("e0554a476A092703abdB3Ef35c80e0D76d32939F");
    let discovered_pool = new_pools
        .iter()
        .find(|p| p.address() == usdc_weth_pool_address)
        .expect("The USDC/WETH 0.01% pool should have been discovered");

    let concrete_pool = discovered_pool
        .as_any()
        .downcast_ref::<UniswapV3Pool<DynProvider>>()
        .expect("Discovered pool should be a UniswapV3Pool");

    assert_eq!(concrete_pool.fee(), 100);
    assert_eq!(concrete_pool.tick_spacing(), 1);
}
