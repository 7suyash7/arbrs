use alloy::sol;
use alloy_primitives::aliases::U24;
use alloy_primitives::{Address, I256, U256, address};
use alloy_provider::{Provider, ProviderBuilder};
use alloy_rpc_types::TransactionRequest;
use alloy_sol_types::SolCall;
use arbrs::TokenLike;
use arbrs::db::DbManager;
use arbrs::pool::uniswap_v3::UniswapV3Pool;
use arbrs::pool::uniswap_v3::{TickInfo, UniswapV3PoolState};
use arbrs::pool::{LiquidityPool, PoolSnapshot};
use arbrs::{
    TokenManager,
    math::v3::{
        sqrt_price_math::{self, MAX_U160},
        swap_math::{self},
        tick::Tick,
        utils::sqrt,
    },
};
use ruint::aliases::U160;
use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::Arc;

const FORK_RPC_URL: &str = "http://127.0.0.1:8545";
const DB_URL: &str = "sqlite::memory:";
const WBTC_WETH_V3_POOL_ADDRESS: Address = address!("CBCdF9626bC03E24f779434178A73a0B4bad62eD");
const WETH_ADDRESS: Address = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
const WBTC_ADDRESS: Address = address!("2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599");
const QUOTER_ADDRESS: Address = address!("b27308f9F90D607463bb33eA1BeBb41C27CE5AB6");
const TEST_BLOCK: u64 = 19000000;
type DynProvider = dyn Provider + Send + Sync;

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

async fn setup() -> (
    Arc<DynProvider>,
    Arc<DbManager>,
    Arc<TokenManager<DynProvider>>,
) {
    let provider = ProviderBuilder::new().connect_http(FORK_RPC_URL.parse().unwrap());
    let provider_arc: Arc<DynProvider> = Arc::new(provider);
    let db_manager = Arc::new(DbManager::new(DB_URL).await.unwrap());
    let token_manager = Arc::new(TokenManager::new(
        provider_arc.clone(),
        1,
        db_manager.clone(),
    ));
    (provider_arc, db_manager, token_manager)
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

    let result_cap_out = swap_math::compute_swap_step(
        U256::from_str("417332158212080721273783715441582").unwrap(),
        U256::from_str("1452870262520218020823638996").unwrap(),
        159344665391607089467575320103,
        I256::from_str("-1").unwrap(),
        1,
    )
    .unwrap();
    assert_eq!(result_cap_out.amount_in, U256::from(2));
    assert_eq!(result_cap_out.amount_out, U256::from(1));
    assert_eq!(result_cap_out.fee_amount, U256::from(1));
    assert_eq!(
        result_cap_out.sqrt_ratio_next_x96,
        U256::from_str("417332158212080721273783715441581").unwrap()
    );

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
async fn test_v3_swap_calculations() {
    let (provider, _db, token_manager) = setup().await;
    let weth = token_manager.get_token(WETH_ADDRESS).await.unwrap();
    let wbtc = token_manager.get_token(WBTC_ADDRESS).await.unwrap();

    let pool = UniswapV3Pool::new(
        WBTC_WETH_V3_POOL_ADDRESS,
        wbtc.clone(),
        weth.clone(),
        3000,
        60,
        provider.clone(),
        None,
    );

    let snapshot = pool.get_snapshot(Some(TEST_BLOCK)).await.unwrap();

    let amount_in_wbtc = U256::from(10_000_000);
    let local_amount_out_weth = pool
        .calculate_tokens_out(&wbtc, &weth, amount_in_wbtc, &snapshot)
        .unwrap();

    let expected_weth_out = U256::from_str("1667334070818084965").unwrap();
    assert_eq!(local_amount_out_weth, expected_weth_out);

    let amount_in_weth = U256::from(10).pow(U256::from(18));
    let local_amount_out_wbtc = pool
        .calculate_tokens_out(&weth, &wbtc, amount_in_weth, &snapshot)
        .unwrap();

    let expected_wbtc_out = U256::from(5961624);
    assert_eq!(local_amount_out_wbtc, expected_wbtc_out);
}

#[tokio::test]
async fn test_v3_exchange_rate_from_sqrt_price() {
    let (_provider, _db, token_manager) = setup().await;
    let weth = token_manager.get_token(WETH_ADDRESS).await.unwrap();
    let wbtc = token_manager.get_token(WBTC_ADDRESS).await.unwrap();
    let pool = UniswapV3Pool::new(
        WBTC_WETH_V3_POOL_ADDRESS,
        wbtc.clone(),
        weth.clone(),
        3000,
        60,
        _provider,
        None,
    );

    pool.update_state_at_block(TEST_BLOCK).await.unwrap();

    let price = pool.absolute_price(&wbtc, &weth).await.unwrap();

    let state = pool.state.read().await;
    let sqrt_price_f64: f64 = state.sqrt_price_x96.to_string().parse().unwrap();
    let q96_f64 = (U256::from(1) << 96u32).to_string().parse::<f64>().unwrap();
    let expected_price = (sqrt_price_f64 / q96_f64).powi(2);

    assert!((price - expected_price).abs() < 1e-9);
}

#[tokio::test]
async fn test_v3_swap_calculations_match_quoter() {
    let (provider, _db, token_manager) = setup().await;
    let weth = token_manager.get_token(WETH_ADDRESS).await.unwrap();
    let wbtc = token_manager.get_token(WBTC_ADDRESS).await.unwrap();
    let pool = UniswapV3Pool::new(
        WBTC_WETH_V3_POOL_ADDRESS,
        wbtc.clone(),
        weth.clone(),
        3000,
        60,
        provider.clone(),
        None,
    );

    let snapshot = pool.get_snapshot(Some(TEST_BLOCK)).await.unwrap();

    let amount_in_wbtc = U256::from(10_000_000);
    let local_amount_out_weth = pool
        .calculate_tokens_out(&wbtc, &weth, amount_in_wbtc, &snapshot)
        .unwrap();

    let quoter_call = IQuoter::quoteExactInputSingleCall {
        tokenIn: wbtc.address(),
        tokenOut: weth.address(),
        fee: U24::from(3000),
        amountIn: amount_in_wbtc,
        sqrtPriceLimitX96: U160::ZERO,
    };
    let request = TransactionRequest::default()
        .to(QUOTER_ADDRESS)
        .input(quoter_call.abi_encode().into());
    let result_bytes = provider
        .call(request)
        .block(TEST_BLOCK.into())
        .await
        .unwrap();
    let onchain_amount_out_weth =
        IQuoter::quoteExactInputSingleCall::abi_decode_returns(&result_bytes).unwrap();

    assert_eq!(local_amount_out_weth, onchain_amount_out_weth);

    let amount_in_weth = U256::from(10).pow(U256::from(18));
    let local_amount_out_wbtc = pool
        .calculate_tokens_out(&weth, &wbtc, amount_in_weth, &snapshot)
        .unwrap();

    let quoter_call_2 = IQuoter::quoteExactInputSingleCall {
        tokenIn: weth.address(),
        tokenOut: wbtc.address(),
        fee: U24::from(3000),
        amountIn: amount_in_weth,
        sqrtPriceLimitX96: U160::ZERO,
    };
    let request_2 = TransactionRequest::default()
        .to(QUOTER_ADDRESS)
        .input(quoter_call_2.abi_encode().into());
    let result_bytes_2 = provider
        .call(request_2)
        .block(TEST_BLOCK.into())
        .await
        .unwrap();
    let onchain_amount_out_wbtc =
        IQuoter::quoteExactInputSingleCall::abi_decode_returns(&result_bytes_2).unwrap();

    assert_eq!(local_amount_out_wbtc, onchain_amount_out_wbtc);
}

#[tokio::test]
async fn test_v3_simulations() {
    let (provider, _db, token_manager) = setup().await;
    let weth = token_manager.get_token(WETH_ADDRESS).await.unwrap();
    let wbtc = token_manager.get_token(WBTC_ADDRESS).await.unwrap();
    let pool = UniswapV3Pool::new(
        WBTC_WETH_V3_POOL_ADDRESS,
        wbtc.clone(),
        weth.clone(),
        3000,
        60,
        provider.clone(),
        None,
    );

    let snapshot = match pool.get_snapshot(Some(TEST_BLOCK)).await.unwrap() {
        PoolSnapshot::UniswapV3(s) => s,
        _ => panic!("Wrong snapshot type"),
    };

    let amount_in_wbtc = U256::from(100_000_000);
    let sim_result = pool
        .simulate_exact_input_swap(&wbtc, &weth, amount_in_wbtc, &snapshot)
        .unwrap();

    let expected_weth_out = pool
        .calculate_tokens_out(
            &wbtc,
            &weth,
            amount_in_wbtc,
            &PoolSnapshot::UniswapV3(snapshot.clone()),
        )
        .unwrap();
    assert_eq!(sim_result.amount0_delta, I256::from_raw(amount_in_wbtc));
    assert_eq!(sim_result.amount1_delta, -I256::from_raw(expected_weth_out));
}
