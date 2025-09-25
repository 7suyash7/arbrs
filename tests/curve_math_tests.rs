#[cfg(test)]
mod curve_tests {
    use alloy_primitives::{Address, U256, address};
    use alloy_provider::{Provider, ProviderBuilder};
    use alloy_rpc_types::TransactionRequest;
    use alloy_sol_types::{SolCall, sol};
    use arbrs::{
        core::token::Token,
        curve::{pool::CurveStableswapPool, registry::CurveRegistry},
        manager::token_manager::TokenManager,
        pool::LiquidityPool, TokenLike,
    };
    use std::sync::Arc;
    use url::Url;
    use itertools::Itertools; // Add this for permutations

    // --- Test Configuration ---
    const FORK_RPC_URL: &str = "http://127.0.0.1:8545"; // Make sure your Anvil fork is running here
    const CURVE_MAINNET_REGISTRY: Address = address!("90E00ACe148ca3b23Ac1bC8C240C2a7Dd9c2d7f5");
    
    // Pool addresses for testing different strategies
    const TRIPOOL_ADDRESS: Address = address!("bEbc44782C7dB0a1A60Cb6fe97d0b483032FF1C7"); // DefaultStrategy
    const RAI3CRV_METAPOOL_ADDRESS: Address = address!("618788357D0EBd8A37e763ADab3bc575D54c2C7d"); // MetapoolStrategy
    const COMPOUND_POOL_ADDRESS: Address = address!("A2B47E3D5c44877cca798226B7B8118F9BFb7A56"); // LendingStrategy
    const AAVE_POOL_ADDRESS: Address = address!("52EA46506B9CC5Ef470C5bf89f17Dc28bB35D85C"); // LendingStrategy
    const UNSCALED_POOL_ADDRESS: Address = address!("04c90C198b2eFF55716079bc06d7CCc4aa4d7512"); // UnscaledStrategy
    const DYNAMIC_FEE_POOL_ADDRESS: Address = address!("EB16Ae0052ed37f479f7fe63849198Df1765a733"); // DynamicFeeStrategy
    const ADMIN_FEE_POOL_ADDRESS: Address = address!("4e0915C88bC70750D68C481540F081fEFaF22273"); // AdminFeeStrategy
    const ORACLE_POOL_ADDRESS: Address = address!("59Ab5a5b5d617E478a2479B0cAD80DA7e2831492"); // OracleStrategy


    type DynProvider = dyn Provider + Send + Sync;

    sol! {
        // ABI for on-chain validation calls
        function get_dy(int128 i, int128 j, uint256 dx) external view returns (uint256);
    }

    /// Helper to set up a pool instance against a forked Anvil environment.
    async fn setup_pool(pool_address: Address) -> Arc<CurveStableswapPool<DynProvider>> {
        let url = Url::parse(FORK_RPC_URL).expect("Failed to parse RPC URL");
        
        // FIXED: Explicitly type the provider as a trait object (`Arc<DynProvider>`) upon creation.
        // This resolves the complex concrete type mismatch.
        let provider: Arc<DynProvider> = Arc::new(ProviderBuilder::new().on_http(url));
        
        // FIXED: Add the missing chain_id argument (1 for mainnet).
        let token_manager = Arc::new(TokenManager::new(provider.clone(), 1));
        let registry = CurveRegistry::new(CURVE_MAINNET_REGISTRY, provider.clone());

        Arc::new(
            CurveStableswapPool::new(pool_address, provider, &token_manager, &registry)
                .await
                .unwrap(),
        )
    }

    /// A reusable test harness that validates all swap pairs for a given pool.
    async fn validate_direct_swaps_for_pool(pool: &CurveStableswapPool<DynProvider>) {
        let tokens = &pool.tokens;
        let provider = &pool.provider;

        for p in tokens.iter().permutations(2) {
            let token_in = p[0];
            let token_out = p[1];
            
            println!(
                "\n--- Validating DIRECT swap: {} -> {} on pool {} ---",
                token_in.symbol(),
                token_out.symbol(),
                pool.address
            );

            let i = tokens.iter().position(|t| t.address() == token_in.address()).unwrap() as i128;
            let j = tokens.iter().position(|t| t.address() == token_out.address()).unwrap() as i128;

            let amount_in = U256::from(100_000) * U256::from(10).pow(U256::from(token_in.decimals()));

            let local_amount_out = pool
                .calculate_tokens_out(token_in, token_out, amount_in)
                .await
                .unwrap();

            let onchain_call = get_dyCall { i, j, dx: amount_in };
            let request = TransactionRequest::default().to(pool.address).input(onchain_call.abi_encode().into());
            let result_bytes = provider.call(request).await.unwrap();
            let onchain_amount_out = get_dyCall::abi_decode_returns(&result_bytes).unwrap();

            println!("Local calculation: {}", local_amount_out);
            println!("On-chain call:     {}", onchain_amount_out);
            
            let difference = if local_amount_out > onchain_amount_out {
                local_amount_out - onchain_amount_out
            } else {
                onchain_amount_out - local_amount_out
            };

            let tolerance = if pool.address == COMPOUND_POOL_ADDRESS || pool.address == AAVE_POOL_ADDRESS {
                // U256::from(100_000_000)
                U256::from(10)
            } else {
                U256::from(10) // A slightly larger default tolerance for complexity
            };

            println!("Difference:        {} (Tolerance: {})", difference, tolerance);

            assert!(
                difference <= tolerance,
                "Swap calculation for {}->{} does not match on-chain result! Difference: {}",
                token_in.symbol(),
                token_out.symbol(),
                difference
            );
        }
}

    /// A placeholder harness for UNDERLYING swaps. We will use this later.
    async fn validate_underlying_swaps_for_pool(pool: &CurveStableswapPool<DynProvider>) {
        // TODO: Implement this test harness to validate swaps between underlying tokens
        // using the `get_dy_underlying` on-chain function.
        unimplemented!("Underlying swap validation not yet implemented");
    }

    #[tokio::test]
    async fn test_default_strategy_tripool() {
        let tripool = setup_pool(TRIPOOL_ADDRESS).await;
        validate_direct_swaps_for_pool(&tripool).await;
    }

    #[tokio::test]
    async fn test_metapool_strategy_rai3crv() {
        let rai_pool = setup_pool(RAI3CRV_METAPOOL_ADDRESS).await;
        validate_direct_swaps_for_pool(&rai_pool).await;
    }

    #[tokio::test]
    async fn test_lending_strategy_compound() {
        let compound_pool = setup_pool(COMPOUND_POOL_ADDRESS).await;
        validate_direct_swaps_for_pool(&compound_pool).await;
    }

    #[tokio::test]
    async fn test_lending_strategy_aave() {
        let aave_pool = setup_pool(AAVE_POOL_ADDRESS).await;
        validate_direct_swaps_for_pool(&aave_pool).await;
    }

    #[tokio::test]
    async fn test_unscaled_strategy() {
        let pool = setup_pool(UNSCALED_POOL_ADDRESS).await;
        validate_direct_swaps_for_pool(&pool).await;
    }

    #[tokio::test]
    async fn test_dynamic_fee_strategy_steth() {
        let pool = setup_pool(DYNAMIC_FEE_POOL_ADDRESS).await;
        validate_direct_swaps_for_pool(&pool).await;
    }

    #[tokio::test]
    async fn test_admin_fee_strategy() {
        let pool = setup_pool(ADMIN_FEE_POOL_ADDRESS).await;
        validate_direct_swaps_for_pool(&pool).await;
    }

    #[tokio::test]
    async fn test_oracle_strategy_rai() {
        // This pool uses its main `get_dy` function for underlying swaps,
        // so we test it with our direct swap validator.
        let pool = setup_pool(ORACLE_POOL_ADDRESS).await;
        validate_direct_swaps_for_pool(&pool).await;
    }
}

// async fn validate_all_swaps_for_pool(pool: &CurveStableswapPool<DynProvider>) {
//     // ... (beginning of the function is unchanged) ...

//     let difference = if local_amount_out > onchain_amount_out {
//         local_amount_out - onchain_amount_out
//     } else {
//         onchain_amount_out - local_amount_out
//     };

//     // --- FINAL FIX ---
//     // Set a wider, but still very small, tolerance for the known complex lending pools.
//     let tolerance = if pool.address == COMPOUND_POOL_ADDRESS || pool.address == AAVE_POOL_ADDRESS {
//         // e.g., A 0.001% tolerance on a 10,000,000,000,000 value is 100,000,000
//         // Our differences are much smaller than this, so a tolerance of 100,000,000 is very safe.
//         U256::from(100_000_000) 
//     } else {
//         U256::from(1) // 1 wei tolerance for all other pools
//     };

//     println!("Difference:        {} (Tolerance: {})", difference, tolerance);

//     assert!(
//         difference <= tolerance,
//         "Swap calculation for {}->{} does not match on-chain result! Difference: {}",
//         token_in.symbol(),
//         token_out.symbol(),
//         difference
//     );
// }