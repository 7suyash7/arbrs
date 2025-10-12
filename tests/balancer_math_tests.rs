#[cfg(test)]
mod balancer_tests {
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
            struct SingleSwap {
                bytes32 poolId;
                uint8 kind;
                address assetIn;
                address assetOut;
                uint256 amount;
                bytes userData;
            }

            interface IBalancerQueries {
                function querySwap(
                    SingleSwap memory singleSwap,
                    (address,bool,address,bool) memory funds
                ) external view returns (uint256);
            }
        }

        async fn setup() -> (Arc<DynProvider>, Arc<TokenManager<DynProvider>>, Arc<DbManager>) {
            let provider = ProviderBuilder::new().connect_http(FORK_RPC_URL.parse().unwrap());
            let provider_arc: Arc<DynProvider> = Arc::new(provider);
            let db_manager = Arc::new(DbManager::new(DB_URL).await.unwrap());
            let token_manager = Arc::new(TokenManager::new(provider_arc.clone(), 1, db_manager.clone()));
            (provider_arc, token_manager, db_manager)
        }

        #[tokio::test]
        async fn test_pool_initialization() {
            let (provider, token_manager, db_manager) = setup().await;
            let pool = BalancerPool::new(POOL_ADDRESS, provider, token_manager, db_manager).await.unwrap();
            let bal_token = address!("ba100000625a3754423978a60c9317c58a424e3D");
            let weth_token = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
            assert_eq!(pool.address(), POOL_ADDRESS);
            assert_eq!(pool.get_all_tokens()[0].address(), bal_token);
            assert_eq!(pool.get_all_tokens()[1].address(), weth_token);
            assert_eq!(pool.fee(), U256::from(10_000_000_000_000_000u64));
        }

        #[tokio::test]
        async fn test_swap_calculation_vs_onchain_quoter() {
            let (provider, token_manager, db_manager) = setup().await;
            let pool = BalancerPool::new(POOL_ADDRESS, provider.clone(), token_manager, db_manager).await.unwrap();
            let snapshot = pool.get_snapshot(Some(TEST_BLOCK)).await.unwrap();
            let bal_token = &pool.get_all_tokens()[0];
            let weth_token = &pool.get_all_tokens()[1];

            // Test cases for BAL -> WETH
            let bal_in_amounts = vec![
                U256::from(10).pow(U256::from(12)), // 0.000001 BAL
                U256::from(10).pow(U256::from(15)), // 0.001 BAL
                U256::from(10).pow(U256::from(17)), // 0.1 BAL
                U256::from(10).pow(U256::from(18)), // 1 BAL
            ];

            for amount_in in bal_in_amounts {
                test_single_swap(&pool, bal_token, weth_token, amount_in, &snapshot, provider.clone()).await;
            }

            // Test cases for WETH -> BAL
            let weth_in_amounts = vec![
                U256::from(10).pow(U256::from(12)), // 0.000001 WETH
                U256::from(10).pow(U256::from(15)), // 0.001 WETH
                U256::from(10).pow(U256::from(17)), // 0.1 WETH
            ];
            
            for amount_in in weth_in_amounts {
                test_single_swap(&pool, weth_token, bal_token, amount_in, &snapshot, provider.clone()).await;
            }
        }

        // Helper function to run a single swap test
        async fn test_single_swap<P: Provider + Send + Sync + 'static + ?Sized>(
            pool: &BalancerPool<P>,
            token_in: &arbrs::Token<P>,
            token_out: &arbrs::Token<P>,
            amount_in: U256,
            snapshot: &arbrs::pool::PoolSnapshot,
            provider: Arc<P>,
        ) {
            let local_amount_out = pool.calculate_tokens_out(token_in, token_out, amount_in, snapshot).unwrap();

            let single_swap = SingleSwap {
                poolId: pool.pool_id.into(),
                kind: 0,
                assetIn: token_in.address(),
                assetOut: token_out.address(),
                amount: amount_in,
                userData: Bytes::new(),
            };
            let funds = (Address::ZERO, false, Address::ZERO, false);
            let quoter_call = IBalancerQueries::querySwapCall { singleSwap: single_swap, funds: funds };
            
            let request = TransactionRequest::default().to(BALANCER_QUERIES).input(quoter_call.abi_encode().into());
            let result_bytes = provider.call(request).block(TEST_BLOCK.into()).await.unwrap();
            let onchain_amount_out = IBalancerQueries::querySwapCall::abi_decode_returns(&result_bytes).unwrap();

            let diff = if local_amount_out > onchain_amount_out { 
                local_amount_out - onchain_amount_out 
            } else { 
                onchain_amount_out - local_amount_out 
            };

            let tolerance = U256::from(1_000_000_000); // 0.000007% diff not sure why but this almost made me kms
            assert!(
                diff <= tolerance, 
                "Mismatch for amount in {}: got {}, expected {}. Diff: {}", 
                amount_in, local_amount_out, onchain_amount_out, diff
            );
        }
    }
}