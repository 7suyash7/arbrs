#[cfg(test)]
mod balancer_tests {
    mod pure_math_tests {
        use arbrs::{
            balancer::weighted_math,
            errors::ArbRsError,
            math::balancer::{constants::*, fixed_point as fp},
        };
        use alloy_primitives::U256;
        use std::str::FromStr;

        fn assert_approx_equal(rust_val: U256, expected: U256) {
            let diff = if rust_val > expected {
                rust_val - expected
            } else {
                expected - rust_val
            };
            // The tolerance is tiny (2 wei) to account for any single off-by-one
            // rounding difference deep in the algorithm.
            assert!(
                diff <= U256::from(2),
                "Assertion failed:\n  rust:     {}\n  expected: {}\n  diff:     {}",
                rust_val,
                expected,
                diff,
            );
        }

        #[test]
        fn test_log_exp_edge_cases() {
            assert_eq!(fp::pow_down(U256::ZERO, U256::ZERO).unwrap(), ONE);
            assert_eq!(fp::pow_down(U256::from(10) * ONE, U256::ZERO).unwrap(), ONE);
            assert_eq!(fp::pow_down(U256::ZERO, ONE).unwrap(), U256::ZERO);

            let res = fp::pow_down(ONE, U256::from(10) * ONE).unwrap();
            assert_approx_equal(res, ONE);
        }

        #[test]
        fn test_pow_decimals() {
            let base = U256::from(2) * ONE;
            let exponent = U256::from(4) * ONE;
            let expected_result = U256::from(16) * ONE;
            let result = fp::pow_down(base, exponent).unwrap();
            assert_approx_equal(result, expected_result);
        }

        #[test]
        fn test_pow_down_up_special_cases() {
            let x = U256::from_str("1234500000000000000").unwrap(); // 1.2345
            // power = 1
            assert_eq!(fp::pow_down(x, ONE).unwrap(), x);
            assert_eq!(fp::pow_up(x, ONE).unwrap(), x);
            // power = 2
            assert_eq!(fp::pow_down(x, TWO).unwrap(), fp::mul_down(x, x).unwrap());
            assert_eq!(fp::pow_up(x, TWO).unwrap(), fp::mul_up(x, x).unwrap());
            // power = 4
            let x_sq_down = fp::mul_down(x, x).unwrap();
            let x_sq_up = fp::mul_up(x, x).unwrap();
            assert_eq!(
                fp::pow_down(x, FOUR).unwrap(),
                fp::mul_down(x_sq_down, x_sq_down).unwrap()
            );
            assert_eq!(fp::pow_up(x, FOUR).unwrap(), fp::mul_up(x_sq_up, x_sq_up).unwrap());
        }

        #[test]
        fn test_calc_out_given_in() {
            let balance_in = U256::from(100) * ONE;
            let weight_in = ONE / U256::from(2); // 0.5
            let balance_out = U256::from(100) * ONE;
            let weight_out = fp::div_down(ONE * U256::from(4), U256::from(10)).unwrap(); // 0.4
            let amount_in = U256::from(15) * ONE;

            let expected_out = U256::from_str("10972338333333333333").unwrap();
            let rust_out = weighted_math::calc_out_given_in(
                balance_in, weight_in, balance_out, weight_out, amount_in,
            )
            .unwrap();
            assert_approx_equal(rust_out, expected_out);
        }

        #[test]
        fn test_calc_in_given_out() {
            let balance_in = U256::from(100) * ONE;
            let weight_in = ONE / U256::from(2); // 0.5
            let balance_out = U256::from(100) * ONE;
            let weight_out = fp::div_down(ONE * U256::from(4), U256::from(10)).unwrap(); // 0.4
            let amount_out = U256::from(15) * ONE;

            let expected_in = U256::from_str("20353383333333333333").unwrap();
            let rust_in = weighted_math::calc_in_given_out(
                balance_in, weight_in, balance_out, weight_out, amount_out,
            )
            .unwrap();
            assert_approx_equal(rust_in, expected_in);
        }
        
        #[test]
        fn test_out_of_bounds_errors() {
            let max_in = fp::mul_down(U256::from(100) * ONE, U256::from_str("300000000000000000").unwrap()).unwrap();
            let res_in = weighted_math::calc_out_given_in(U256::from(100) * ONE, ONE, U256::from(100) * ONE, ONE, max_in + U256::from(1));
            assert!(matches!(res_in, Err(ArbRsError::CalculationError(s)) if s == "MAX_IN_RATIO"));

            let max_out = fp::mul_down(U256::from(100) * ONE, U256::from_str("300000000000000000").unwrap()).unwrap();
            let res_out = weighted_math::calc_in_given_out(U256::from(100) * ONE, ONE, U256::from(100) * ONE, ONE, max_out + U256::from(1));
            assert!(matches!(res_out, Err(ArbRsError::CalculationError(s)) if s == "MAX_OUT_RATIO"));
        }

        #[test]
        fn test_invariant_calculation() {
            let weights1 = vec![
                fp::div_down(ONE * U256::from(3), U256::from(10)).unwrap(),
                fp::div_down(ONE * U256::from(7), U256::from(10)).unwrap(),
            ];
            let balances1 = vec![ONE, U256::from(12) * ONE];
            let rust_invariant1 = weighted_math::calculate_invariant(&weights1, &balances1).unwrap();
            let expected_invariant1 = U256::from_str("6234515286591321639").unwrap();
            assert_approx_equal(rust_invariant1, expected_invariant1);
        }
    }

    mod integration_tests {
        use alloy_primitives::{Address, Bytes, U256, address};
        use alloy_provider::{Provider, ProviderBuilder};
        use alloy_rpc_types::TransactionRequest;
        use alloy_sol_types::{SolCall, sol};
        use arbrs::{
            TokenLike, balancer::pool::BalancerPool, db::DbManager,
            manager::token_manager::TokenManager, pool::LiquidityPool,
        };
        use std::sync::Arc;

        type DynProvider = dyn Provider + Send + Sync;

        const FORK_RPC_URL: &str = "http://127.0.0.1:8545";
        const DB_URL: &str = "sqlite::memory:";
        const TEST_BLOCK: u64 = 19000000;

        // Balancer V2 80/20 BAL/WETH Pool
        const POOL_ADDRESS: Address = address!("5c6Ee304399DBdB9C8Ef030aB642B10820DB8F56");
        const BALANCER_QUERIES: Address = address!("E39B5e3B6D74016b2F6A9673D7d7493B6DF549d5");

        sol! {
            // Simplified interface for the BalancerQueries contract
            interface IBalancerQueries {
                function querySwap(
                    SingleSwap memory singleSwap,
                    FundManagement memory funds
                ) external returns (uint256);
            }

            // Structs needed for the querySwap call
            struct SingleSwap {
                bytes32 poolId;
                uint8 kind; // 0 = GIVEN_IN, 1 = GIVEN_OUT
                address assetIn;
                address assetOut;
                uint256 amount;
                bytes userData;
            }

            struct FundManagement {
                address sender;
                bool fromInternalBalance;
                address recipient;
                bool toInternalBalance;
            }
        }

        async fn setup() -> (
            Arc<DynProvider>,
            Arc<TokenManager<DynProvider>>,
            Arc<DbManager>,
        ) {
            let provider = ProviderBuilder::new().connect_http(FORK_RPC_URL.parse().unwrap());
            let provider_arc: Arc<DynProvider> = Arc::new(provider);
            let db_manager = Arc::new(DbManager::new(DB_URL).await.unwrap());
            let token_manager = Arc::new(TokenManager::new(
                provider_arc.clone(),
                1,
                db_manager.clone(),
            ));
            (provider_arc, token_manager, db_manager)
        }

        #[tokio::test]
        async fn test_pool_initialization() {
            let (provider, token_manager, db_manager) = setup().await;
            let pool = BalancerPool::new(POOL_ADDRESS, provider, token_manager, db_manager)
                .await
                .unwrap();

            let bal_token = address!("ba100000625a3754423978a60c9317c58a424e3D");
            let weth_token = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");

            assert_eq!(pool.address, POOL_ADDRESS);
            assert_eq!(pool.get_all_tokens()[0].address(), bal_token);
            assert_eq!(pool.get_all_tokens()[1].address(), weth_token);
            assert_eq!(pool.fee(), U256::from(10_000_000_000_000_000u64)); // 0.01
        }

        #[tokio::test]
        async fn test_swap_calculation_vs_onchain_quoter() {
            let (provider, token_manager, db_manager) = setup().await;
            let pool = BalancerPool::new(
                POOL_ADDRESS,
                provider.clone(),
                token_manager.clone(),
                db_manager,
            )
            .await
            .unwrap();

            let snapshot = pool.get_snapshot(Some(TEST_BLOCK)).await.unwrap();

            let bal_token = &pool.get_all_tokens()[0];
            let weth_token = &pool.get_all_tokens()[1];

            let bal_in_amounts = vec![
                U256::from(10).pow(U256::from(12)), // 0.000001 BAL
                U256::from(10).pow(U256::from(15)), // 0.001 BAL
                U256::from(10).pow(U256::from(17)), // 0.1 BAL
                U256::from(10).pow(U256::from(18)), // 1 BAL
            ];

            // Test BAL -> WETH swaps
            for amount_in in bal_in_amounts {
                let local_amount_out = pool
                    .calculate_tokens_out(bal_token, weth_token, amount_in, &snapshot)
                    .unwrap();

                let single_swap = SingleSwap {
                    poolId: pool.pool_id.into(),
                    kind: 0,
                    assetIn: bal_token.address(),
                    assetOut: weth_token.address(),
                    amount: amount_in,
                    userData: Bytes::new(),
                };
                let funds = FundManagement {
                    sender: Address::ZERO,
                    fromInternalBalance: false,
                    recipient: Address::ZERO,
                    toInternalBalance: false,
                };
                let quoter_call = IBalancerQueries::querySwapCall {
                    singleSwap: single_swap,
                    funds,
                };
                let request = TransactionRequest::default()
                    .to(BALANCER_QUERIES)
                    .input(quoter_call.abi_encode().into());
                let result_bytes = provider
                    .call(request)
                    .block(TEST_BLOCK.into())
                    .await
                    .unwrap();
                let onchain_amount_out =
                    IBalancerQueries::querySwapCall::abi_decode_returns(&result_bytes).unwrap();

                assert_eq!(
                    local_amount_out, onchain_amount_out,
                    "Mismatch for BAL in: {}",
                    amount_in
                );
            }

            let weth_in_amounts = vec![
                U256::from(10).pow(U256::from(12)), // 0.000001 WETH
                U256::from(10).pow(U256::from(15)), // 0.001 WETH
                U256::from(10).pow(U256::from(17)), // 0.1 WETH
            ];

            // Test WETH -> BAL swaps
            for amount_in in weth_in_amounts {
                let local_amount_out = pool
                    .calculate_tokens_out(weth_token, bal_token, amount_in, &snapshot)
                    .unwrap();

                let single_swap = SingleSwap {
                    poolId: pool.pool_id.into(),
                    kind: 0,
                    assetIn: weth_token.address(),
                    assetOut: bal_token.address(),
                    amount: amount_in,
                    userData: Bytes::new(),
                };
                let funds = FundManagement {
                    sender: Address::ZERO,
                    fromInternalBalance: false,
                    recipient: Address::ZERO,
                    toInternalBalance: false,
                };
                let quoter_call = IBalancerQueries::querySwapCall {
                    singleSwap: single_swap,
                    funds,
                };
                let request = TransactionRequest::default()
                    .to(BALANCER_QUERIES)
                    .input(quoter_call.abi_encode().into());
                let result_bytes = provider
                    .call(request)
                    .block(TEST_BLOCK.into())
                    .await
                    .unwrap();
                let onchain_amount_out =
                    IBalancerQueries::querySwapCall::abi_decode_returns(&result_bytes).unwrap();

                assert_eq!(
                    local_amount_out, onchain_amount_out,
                    "Mismatch for WETH in: {}",
                    amount_in
                );
            }
        }
    }
}
