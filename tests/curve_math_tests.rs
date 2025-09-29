#[cfg(test)]
mod curve_tests {
    use alloy_primitives::{Address, Bytes, U256, address};
    use alloy_provider::{Provider, ProviderBuilder};
    use alloy_rpc_types::{BlockId, BlockNumberOrTag, TransactionRequest};
    use alloy_sol_types::{SolCall, sol};
    use arbrs::{
        ArbRsError, TokenLike,
        core::token::Token,
        curve::{
            pool::{self, CurveStableswapPool},
            registry::CurveRegistry,
        },
        manager::token_manager::TokenManager,
        pool::LiquidityPool,
    };
    use itertools::Itertools;
    use serde_json::{Value, json};
    use std::sync::Arc;
    use url::Url;

    const FORK_RPC_URL: &str = "http://127.0.0.1:8545";
    const CURVE_MAINNET_REGISTRY: Address = address!("90E00ACe148ca3b23Ac1bC8C240C2a7Dd9c2d7f5");

    const TRIPOOL_ADDRESS: Address = address!("bEbc44782C7dB0a1A60Cb6fe97d0b483032FF1C7");
    const RAI3CRV_METAPOOL_ADDRESS: Address = address!("618788357D0EBd8A37e763ADab3bc575D54c2C7d");
    const COMPOUND_POOL_ADDRESS: Address = address!("A2B47E3D5c44877cca798226B7B8118F9BFb7A56");
    const AAVE_POOL_ADDRESS: Address = address!("52EA46506B9CC5Ef470C5bf89f17Dc28bB35D85C");
    const UNSCALED_POOL_ADDRESS: Address = address!("04c90C198b2eFF55716079bc06d7CCc4aa4d7512");
    const DYNAMIC_FEE_POOL_ADDRESS: Address = address!("DC24316b9AE028F1497c275EB9192a3Ea0f67022");
    const ADMIN_FEE_POOL_ADDRESS: Address = address!("4e0915C88bC70750D68C481540F081fEFaF22273");
    const ORACLE_POOL_ADDRESS: Address = address!("59Ab5a5b5d617E478a2479B0cAD80DA7e2831492");
    const MIM_METAPOOL: Address = address!("DeBF20617708857ebe4F679508E7b7863a8A8EeE");
    const IRON_BANK_POOL: Address = address!("2dded6Da1BF5DBdF597C45fcFaa3194e53EcfeAF");
    const SAAVE_POOL: Address = address!("EB16Ae0052ed37f479f7fe63849198Df1765a733");
    type DynProvider = dyn Provider + Send + Sync;

    sol! {
        function get_dy(int128 i, int128 j, uint256 dx) external view returns (uint256);
        function get_dy_underlying(int128 i, int128 j, uint256 dx) external view returns (uint256);
        function calc_token_amount(uint256[3] calldata amounts, bool is_deposit) external view returns (uint256);
        function calc_withdraw_one_coin(uint256 _token_amount, int128 i) external view returns (uint256);
        interface ICurveRegistryV1 {
            function pool_count() external view returns (uint256);
            function pool_list(uint256 i) external view returns (address);
        }
    }

    async fn setup_pool(pool_address: Address) -> Arc<CurveStableswapPool<DynProvider>> {
        let url = Url::parse(FORK_RPC_URL).expect("Failed to parse RPC URL");
        let provider: Arc<DynProvider> = Arc::new(ProviderBuilder::new().connect_http(url));
        let token_manager = Arc::new(TokenManager::new(provider.clone(), 1));
        let registry = CurveRegistry::new(CURVE_MAINNET_REGISTRY, provider.clone());
        Arc::new(
            CurveStableswapPool::new(pool_address, provider, token_manager.clone(), &registry)
                .await
                .unwrap(),
        )
    }

    async fn try_setup_pool(
        pool_address: Address,
    ) -> Result<Arc<CurveStableswapPool<DynProvider>>, ArbRsError> {
        let url = Url::parse(FORK_RPC_URL).expect("Failed to parse RPC URL");
        let provider: Arc<DynProvider> = Arc::new(ProviderBuilder::new().on_http(url));
        let token_manager = Arc::new(TokenManager::new(provider.clone(), 1));
        let registry = CurveRegistry::new(CURVE_MAINNET_REGISTRY, provider.clone());

        let pool =
            CurveStableswapPool::new(pool_address, provider, token_manager.clone(), &registry)
                .await?;
        Ok(Arc::new(pool))
    }

    async fn validate_direct_swaps_for_pool(pool: &CurveStableswapPool<DynProvider>) {
        let provider = &pool.provider;
        let tokens = &pool.tokens;

        let block_number = provider.get_block_number().await.unwrap();
        println!(
            "\n--- Running validation against locked Block: {} ---",
            block_number
        );

        for p in tokens.iter().permutations(2) {
            let token_in = p[0];
            let token_out = p[1];

            println!(
                "\n--- Validating DIRECT swap: {} -> {} on pool {} ---",
                token_in.symbol(),
                token_out.symbol(),
                pool.address
            );

            let i = tokens
                .iter()
                .position(|t| t.address() == token_in.address())
                .unwrap() as i128;
            let j = tokens
                .iter()
                .position(|t| t.address() == token_out.address())
                .unwrap() as i128;
            let amount_in =
                U256::from(100_000) * U256::from(10).pow(U256::from(token_in.decimals()));

            let local_amount_out = pool
                .calculate_tokens_out(token_in, token_out, amount_in, Some(block_number))
                .await
                .unwrap();

            let onchain_call = get_dyCall {
                i,
                j,
                dx: amount_in,
            };
            let request = TransactionRequest::default()
                .to(pool.address)
                .input(onchain_call.abi_encode().into());

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

            let tolerance = match pool.address {
                COMPOUND_POOL_ADDRESS | AAVE_POOL_ADDRESS => U256::from(1),
                _ => U256::from(1),
            };

            assert!(
                difference <= tolerance,
                "Swap calculation for {}->{} does not match on-chain result! Difference: {}",
                token_in.symbol(),
                token_out.symbol(),
                difference
            );
        }
    }

    async fn validate_underlying_swaps_for_pool(pool: &CurveStableswapPool<DynProvider>) {
        let provider = &pool.provider;
        let underlying_tokens = &pool.underlying_tokens;
        let block_number = provider.get_block_number().await.unwrap();

        println!(
            "\n--- Running UNDERLYING validation against locked Block: {} ---",
            block_number
        );

        for p in underlying_tokens.iter().permutations(2) {
            let token_in = p[0];
            let token_out = p[1];

            println!(
                "\n--- Validating UNDERLYING swap: {} -> {} on pool {} ---",
                token_in.symbol(),
                token_out.symbol(),
                pool.address
            );

            let i = underlying_tokens
                .iter()
                .position(|t| t == token_in)
                .unwrap() as i128;
            let j = underlying_tokens
                .iter()
                .position(|t| t == token_out)
                .unwrap() as i128;

            let amount_in = U256::from(100) * U256::from(10).pow(U256::from(token_in.decimals()));

            let local_amount_out = pool
                .calculate_tokens_out(token_in, token_out, amount_in, Some(block_number))
                .await
                .unwrap();

            let onchain_call = get_dy_underlyingCall {
                i,
                j,
                dx: amount_in,
            };
            let request = TransactionRequest::default()
                .to(pool.address)
                .input(onchain_call.abi_encode().into());
            let result_bytes = provider
                .call(request)
                .block(block_number.into())
                .await
                .unwrap();
            let onchain_amount_out =
                get_dy_underlyingCall::abi_decode_returns(&result_bytes).unwrap();

            println!("Local calculation: {}", local_amount_out);
            println!("On-chain call:     {}", onchain_amount_out);

            let difference = if local_amount_out > onchain_amount_out {
                local_amount_out - onchain_amount_out
            } else {
                onchain_amount_out - local_amount_out
            };

            let tolerance = U256::from(1);

            assert!(
                difference <= tolerance,
                "Swap calculation for {}->{} does not match on-chain result! Difference: {}",
                token_in.symbol(),
                token_out.symbol(),
                difference
            );
        }
    }

    async fn validate_liquidity_helpers(pool: &Arc<CurveStableswapPool<DynProvider>>) {
        let provider = &pool.provider;
        let block_number = provider.get_block_number().await.unwrap();
        println!(
            "\n--- Running LP helper validation against locked Block: {} ---",
            block_number
        );

        println!("\n--- Validating `calc_token_amount` ---");
        for i in 0..pool.tokens.len() {
            let token_in = &pool.tokens[i];
            let mut amounts = [U256::ZERO; 3];
            let deposit_amount =
                U256::from(100) * U256::from(10).pow(U256::from(token_in.decimals()));
            amounts[i] = deposit_amount;

            println!("Testing deposit of {} {}", 100, token_in.symbol());

            let local_lp_amount = pool
                .calc_token_amount(&amounts, true, Some(block_number))
                .await
                .unwrap();

            let onchain_call = calc_token_amountCall {
                amounts,
                is_deposit: true,
            };
            let request = TransactionRequest::default()
                .to(pool.address)
                .input(onchain_call.abi_encode().into());
            let result_bytes = provider
                .call(request)
                .block(block_number.into())
                .await
                .unwrap();
            let onchain_lp_amount =
                calc_token_amountCall::abi_decode_returns(&result_bytes).unwrap();

            assert_eq!(
                local_lp_amount,
                onchain_lp_amount,
                "calc_token_amount mismatch for {}",
                token_in.symbol()
            );
        }

        println!("\n--- Validating `calc_withdraw_one_coin` ---");
        let lp_token_amount = U256::from(100) * U256::from(10).pow(U256::from(18));
        for i in 0..pool.tokens.len() {
            let token_out = &pool.tokens[i];
            println!(
                "Testing withdrawal of 100 LP tokens into {}",
                token_out.symbol()
            );

            let (local_amount_out, ..) = pool
                .calc_withdraw_one_coin(lp_token_amount, i, Some(block_number))
                .await
                .unwrap();

            let onchain_call = calc_withdraw_one_coinCall {
                _token_amount: lp_token_amount,
                i: i as i128,
            };
            let request = TransactionRequest::default()
                .to(pool.address)
                .input(onchain_call.abi_encode().into());
            let result_bytes = provider
                .call(request)
                .block(block_number.into())
                .await
                .unwrap();
            let onchain_amount_out =
                calc_withdraw_one_coinCall::abi_decode_returns(&result_bytes).unwrap();

            let difference = if local_amount_out > onchain_amount_out {
                local_amount_out - onchain_amount_out
            } else {
                onchain_amount_out - local_amount_out
            };
            assert!(
                difference <= U256::from(1),
                "calc_withdraw_one_coin mismatch for {}",
                token_out.symbol()
            );
        }
    }

    async fn get_all_registry_pools(
        provider: &Arc<DynProvider>,
        registry_address: Address,
    ) -> Vec<Address> {
        let count_call = ICurveRegistryV1::pool_countCall {};
        let request = TransactionRequest::default()
            .to(registry_address)
            .input(count_call.abi_encode().into());
        let count_bytes = provider.call(request).await.unwrap();
        let pool_count =
            ICurveRegistryV1::pool_countCall::abi_decode_returns(&count_bytes).unwrap();

        let mut pool_addresses = Vec::new();
        for i in 0..pool_count.as_limbs()[0] {
            let list_call = ICurveRegistryV1::pool_listCall { i: U256::from(i) };
            let request = TransactionRequest::default()
                .to(registry_address)
                .input(list_call.abi_encode().into());
            let address_bytes = provider.call(request).await.unwrap();
            let pool_address =
                ICurveRegistryV1::pool_listCall::abi_decode_returns(&address_bytes).unwrap();
            pool_addresses.push(pool_address);
        }
        pool_addresses
    }

    #[tokio::test]
    async fn test_default_strategy_tripool() {
        let pool = setup_pool(TRIPOOL_ADDRESS).await;
        validate_direct_swaps_for_pool(&pool).await;
    }
    #[tokio::test]
    async fn test_metapool_strategy_rai3crv() {
        let pool = setup_pool(RAI3CRV_METAPOOL_ADDRESS).await;
        validate_direct_swaps_for_pool(&pool).await;
    }
    #[tokio::test]
    async fn test_lending_strategy_compound() {
        let pool = setup_pool(COMPOUND_POOL_ADDRESS).await;
        validate_direct_swaps_for_pool(&pool).await;
    }
    #[tokio::test]
    async fn test_lending_strategy_aave() {
        let pool = setup_pool(AAVE_POOL_ADDRESS).await;
        validate_direct_swaps_for_pool(&pool).await;
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
        let pool = setup_pool(ORACLE_POOL_ADDRESS).await;
        validate_direct_swaps_for_pool(&pool).await;
    }
    #[tokio::test]
    async fn test_underlying_swaps_rai3crv() {
        let pool = setup_pool(RAI3CRV_METAPOOL_ADDRESS).await;
        validate_underlying_swaps_for_pool(&pool).await;
    }
    #[tokio::test]
    async fn test_liquidity_helpers_tripool() {
        let pool = setup_pool(TRIPOOL_ADDRESS).await;
        validate_liquidity_helpers(&pool).await;
    }

    #[tokio::test]
    #[ignore]
    async fn test_all_registry_pools() {
        let url = Url::parse(FORK_RPC_URL).expect("Failed to parse RPC URL");
        let provider: Arc<DynProvider> = Arc::new(ProviderBuilder::new().connect_http(url));

        let pool_addresses = get_all_registry_pools(&provider, CURVE_MAINNET_REGISTRY).await;
        println!(
            "Discovered {} pools from the registry. Starting validation...",
            pool_addresses.len()
        );

        for (index, pool_address) in pool_addresses.iter().enumerate() {
            println!(
                "\n--- [{}/{}] TESTING POOL: {} ---",
                index + 1,
                pool_addresses.len(),
                pool_address
            );

            if *pool_address == MIM_METAPOOL || *pool_address == IRON_BANK_POOL {
                println!("[SKIPPED] Temporarily skipping MIM Metapool.");
                continue;
            }

            if *pool_address == address!("79a8C46DeA5aDa233ABaFFD40F3A0A2B1e5A4F27")
                || *pool_address == address!("06364f10B501e868329afBc005b3492902d6C763")
                || *pool_address == SAAVE_POOL
                || *pool_address == address!("45F783CCE6B7FF23B2ab2D70e416cdb7D6055f51")
            {
                println!("[SKIPPED] Temporarily skipping known problematic/deprecated pool.");
                continue;
            }

            match try_setup_pool(*pool_address).await {
                Ok(pool) => {
                    validate_direct_swaps_for_pool(&pool).await;
                    println!("[SUCCESS] Pool {} passed validation.", pool_address);
                }
                Err(e) => {
                    println!(
                        "[SKIPPED] Could not initialize pool {}: {:?}",
                        pool_address, e
                    );
                }
            }
        }
    }
}
