#[cfg(test)]
mod curve_tests {
    use alloy_primitives::{address, Address, Bytes, U256};
    use alloy_provider::{Provider, ProviderBuilder};
    use alloy_rpc_types::{BlockId, BlockNumberOrTag, TransactionRequest};
    use alloy_sol_types::{SolCall, sol};
    use arbrs::{
        core::token::Token,
        curve::{pool::CurveStableswapPool, registry::CurveRegistry},
        manager::token_manager::TokenManager,
        pool::LiquidityPool, TokenLike,
    };
    use serde_json::{json, Value};
    use std::sync::Arc;
    use url::Url;
    use itertools::Itertools;

    const FORK_RPC_URL: &str = "http://127.0.0.1:8545";
    const CURVE_MAINNET_REGISTRY: Address = address!("90E00ACe148ca3b23Ac1bC8C240C2a7Dd9c2d7f5");
    
    // Pool addresses for testing different strategies
    const TRIPOOL_ADDRESS: Address = address!("bEbc44782C7dB0a1A60Cb6fe97d0b483032FF1C7");
    const RAI3CRV_METAPOOL_ADDRESS: Address = address!("618788357D0EBd8A37e763ADab3bc575D54c2C7d");
    const COMPOUND_POOL_ADDRESS: Address = address!("A2B47E3D5c44877cca798226B7B8118F9BFb7A56");
    const AAVE_POOL_ADDRESS: Address = address!("52EA46506B9CC5Ef470C5bf89f17Dc28bB35D85C");
    const UNSCALED_POOL_ADDRESS: Address = address!("04c90C198b2eFF55716079bc06d7CCc4aa4d7512");
    const DYNAMIC_FEE_POOL_ADDRESS: Address = address!("EB16Ae0052ed37f479f7fe63849198Df1765a733");
    const ADMIN_FEE_POOL_ADDRESS: Address = address!("4e0915C88bC70750D68C481540F081fEFaF22273");
    const ORACLE_POOL_ADDRESS: Address = address!("59Ab5a5b5d617E478a2479B0cAD80DA7e2831492");

    type DynProvider = dyn Provider + Send + Sync;

    sol! {
        function get_dy(int128 i, int128 j, uint256 dx) external view returns (uint256);
    }

    async fn setup_pool(pool_address: Address) -> Arc<CurveStableswapPool<DynProvider>> {
        let url = Url::parse(FORK_RPC_URL).expect("Failed to parse RPC URL");
        let provider: Arc<DynProvider> = Arc::new(ProviderBuilder::new().connect_http(url));
        let token_manager = Arc::new(TokenManager::new(provider.clone(), 1));
        let registry = CurveRegistry::new(CURVE_MAINNET_REGISTRY, provider.clone());
        Arc::new(CurveStableswapPool::new(pool_address, provider, token_manager.clone(), &registry).await.unwrap())
    }

    async fn validate_direct_swaps_for_pool(pool: &CurveStableswapPool<DynProvider>) {
    let provider = &pool.provider;
    let tokens = &pool.tokens;

    let block_number = provider.get_block_number().await.unwrap();
    println!("\n--- Running validation against locked Block: {} ---", block_number);

    for p in tokens.iter().permutations(2) {
        let token_in = p[0];
        let token_out = p[1];
        
        println!("\n--- Validating DIRECT swap: {} -> {} on pool {} ---", token_in.symbol(), token_out.symbol(), pool.address);

        let i = tokens.iter().position(|t| t.address() == token_in.address()).unwrap() as i128;
        let j = tokens.iter().position(|t| t.address() == token_out.address()).unwrap() as i128;
        let amount_in = U256::from(100_000) * U256::from(10).pow(U256::from(token_in.decimals()));

        // 1. Run local calculation against the LOCKED block number
        let local_amount_out = pool
            .calculate_tokens_out(token_in, token_out, amount_in, Some(block_number))
            .await
            .unwrap();

        let onchain_call = get_dyCall { i, j, dx: amount_in };
        let request = TransactionRequest::default().to(pool.address).input(onchain_call.abi_encode().into());

        let _params = json!([request, BlockId::from(block_number)]);
        let result_bytes = provider
            .call(request)
            .block(block_number.into())
            .await
            .unwrap();
        
        let onchain_amount_out = get_dyCall::abi_decode_returns(&result_bytes).unwrap();

        println!("Local calculation: {}", local_amount_out);
        println!("On-chain call:     {}", onchain_amount_out);
        
        let difference = if local_amount_out > onchain_amount_out {
            local_amount_out - onchain_amount_out
        } else {
            onchain_amount_out - local_amount_out
        };

        // Set a tolerance for pools with known precision differences
        let tolerance = match pool.address {
            COMPOUND_POOL_ADDRESS | AAVE_POOL_ADDRESS => U256::from(0),
            ADMIN_FEE_POOL_ADDRESS | DYNAMIC_FEE_POOL_ADDRESS | ORACLE_POOL_ADDRESS | RAI3CRV_METAPOOL_ADDRESS => U256::from(0),
            _ => U256::from(1),
        };
    //     let tolerance = match pool.address {
    //     // For the complex lending pools, we accept a slightly larger (but still tiny) tolerance
    //     COMPOUND_POOL_ADDRESS | AAVE_POOL_ADDRESS => U256::from(50000),
        
    //     // For all other standard pools, a difference of 1 wei is acceptable due to the "-1" safety margin.
    //     _ => U256::from(1),
    // };


        assert!(
            difference <= tolerance,
            "Swap calculation for {}->{} does not match on-chain result! Difference: {}",
            token_in.symbol(),
            token_out.symbol(),
            difference
        );
    }
}

    #[tokio::test] async fn test_default_strategy_tripool() { let pool = setup_pool(TRIPOOL_ADDRESS).await; validate_direct_swaps_for_pool(&pool).await; }
    #[tokio::test] async fn test_metapool_strategy_rai3crv() { let pool = setup_pool(RAI3CRV_METAPOOL_ADDRESS).await; validate_direct_swaps_for_pool(&pool).await; }
    #[tokio::test] async fn test_lending_strategy_compound() { let pool = setup_pool(COMPOUND_POOL_ADDRESS).await; validate_direct_swaps_for_pool(&pool).await; }
    #[tokio::test] async fn test_lending_strategy_aave() { let pool = setup_pool(AAVE_POOL_ADDRESS).await; validate_direct_swaps_for_pool(&pool).await; }
    #[tokio::test] async fn test_unscaled_strategy() { let pool = setup_pool(UNSCALED_POOL_ADDRESS).await; validate_direct_swaps_for_pool(&pool).await; }
    #[tokio::test] async fn test_dynamic_fee_strategy_steth() { let pool = setup_pool(DYNAMIC_FEE_POOL_ADDRESS).await; validate_direct_swaps_for_pool(&pool).await; }
    // #[tokio::test] async fn test_admin_fee_strategy() { let pool = setup_pool(ADMIN_FEE_POOL_ADDRESS).await; validate_direct_swaps_for_pool(&pool).await; }
    #[tokio::test]
async fn test_admin_fee_strategy() {
    let pool = setup_pool(ADMIN_FEE_POOL_ADDRESS).await;
    let tokens = &pool.tokens;
    
    let token_in = tokens.iter().find(|t| t.symbol() == "USDC").unwrap();
    let token_out = tokens.iter().find(|t| t.symbol() == "USDT").unwrap();

    let i = tokens.iter().position(|t| t.address() == token_in.address()).unwrap() as i128;
    let j = tokens.iter().position(|t| t.address() == token_out.address()).unwrap() as i128;
    let amount_in = U256::from(100_000) * U256::from(10).pow(U256::from(token_in.decimals()));

    // --- 1. Run our local calculation (with logging) ---
    println!("\n--- RUNNING LOCAL CALCULATION ---");
    pool.calculate_tokens_out(token_in, token_out, amount_in, Some(21100000)).await.unwrap();

    // --- 2. Trace the on-chain calculation ---
    println!("\n--- TRACING ON-CHAIN CALL ---");
    let onchain_call = get_dyCall { i, j, dx: amount_in };
    let request = TransactionRequest::default().to(pool.address).input(onchain_call.abi_encode().into());
    
    // Create a new, concrete provider just for tracing to avoid Sized issues.
    let url = Url::parse(FORK_RPC_URL).expect("Failed to parse RPC URL");
    let trace_provider = ProviderBuilder::new().connect_http(url);

    let tracer_config = json!({"tracer": "callTracer"});
    let params = json!([request, BlockId::from(21100000), tracer_config]);
    
    let trace: Value = trace_provider
        .raw_request("debug_traceCall".into(), params)
        .await
        .unwrap();

    println!("\n--- TRACE OUTPUT ---");
    println!("{}", serde_json::to_string_pretty(&trace).unwrap());
    
    panic!("Trace complete. Please send the full log output.");
}
    #[tokio::test] async fn test_oracle_strategy_rai() { let pool = setup_pool(ORACLE_POOL_ADDRESS).await;
    validate_direct_swaps_for_pool(&pool).await; }
}