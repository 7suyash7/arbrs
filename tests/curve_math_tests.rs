#[cfg(test)]
mod math_tests {
    use alloy_primitives::{Address, Bytes, TxKind, U256, address};
    use alloy_provider::{Provider, ProviderBuilder};
    use alloy_rpc_types::TransactionRequest;
    use alloy_sol_types::{SolCall, sol};
    use arbrs::curve::math::{a, get_d, get_dy};
    use std::sync::Arc;
    use url::Url;

    const FORK_RPC_URL: &str = "http://127.0.0.1:8545";
    type DynProvider = dyn Provider + Send + Sync;

    sol! {
        function get_dy(int128 i, int128 j, uint256 dx) external view returns (uint256);
        function balances(uint256 i) external view returns (uint256);
        function initial_A() external view returns (uint256);
        function future_A() external view returns (uint256);
        function initial_A_time() external view returns (uint256);
        function future_A_time() external view returns (uint256);
    }

    async fn setup_provider() -> Arc<DynProvider> {
        let url = Url::parse(FORK_RPC_URL).expect("Failed to parse RPC URL");
        Arc::new(ProviderBuilder::new().connect_http(url))
    }

    #[test]
    fn test_get_d_empty_pool() {
        let xp = vec![U256::ZERO, U256::ZERO, U256::ZERO];
        let amp = U256::from(1000);
        let d = get_d(&xp, amp).unwrap();
        assert_eq!(d, U256::ZERO, "D should be 0 for an empty pool");
    }

    #[tokio::test]
    async fn test_tripool_calculations() {
        let provider = setup_provider().await;
        let tripool_address = address!("bEbc44782C7dB0a1A60Cb6fe97d0b483032FF1C7");

        let amp = U256::from(2000);
        let fee = U256::from(3000000);
        let n_coins = 3;
        let rates = vec![
            U256::from(10).pow(U256::from(18)),
            U256::from(10).pow(U256::from(30)),
            U256::from(10).pow(U256::from(30)),
        ];

        let mut balances = Vec::with_capacity(n_coins);
        for i in 0..n_coins {
            let balance_call = balancesCall { i: U256::from(i) };
            let request = TransactionRequest {
                to: Some(TxKind::Call(tripool_address)),
                input: Some(Bytes::from(balance_call.abi_encode())).into(),
                ..Default::default()
            };
            let result_bytes = provider.call(request).await.unwrap();
            let balance = balancesCall::abi_decode_returns(&result_bytes).unwrap();
            balances.push(balance);
            balances.push(balance);
        }

        let i = 0;
        let j = 1;
        let dx = U256::from(1_000_000) * U256::from(10).pow(U256::from(18)); // 1,000,000 DAI

        let rust_dy = get_dy(i, j, dx, &balances, amp, fee, &rates).unwrap();

        let onchain_dy_call = get_dyCall {
            i: i as i128,
            j: j as i128,
            dx,
        };

        let request = TransactionRequest {
            to: Some(TxKind::Call(tripool_address)),
            input: Some(Bytes::from(onchain_dy_call.abi_encode())).into(),
            ..Default::default()
        };
        let result_bytes = provider.call(request).await.unwrap();
        let onchain_dy = get_dyCall::abi_decode_returns(&result_bytes).unwrap();

        let difference = if rust_dy > onchain_dy {
            rust_dy - onchain_dy
        } else {
            onchain_dy - rust_dy
        };
        assert!(
            difference <= U256::from(1),
            "Mismatch between Rust ({}) and on-chain ({}) calculation for 3pool",
            rust_dy,
            onchain_dy
        );
    }

    #[tokio::test]
    async fn test_a_ramping() {
        let provider = setup_provider().await;
        let tripool_address = address!("bEbc44782C7dB0a1A60Cb6fe97d0b483032FF1C7");

        let initial_a_call = initial_ACall {};
        let req = TransactionRequest {
            to: Some(TxKind::Call(tripool_address)),
            input: Some(Bytes::from(initial_a_call.abi_encode())).into(),
            ..Default::default()
        };
        let initial_a =
            initial_ACall::abi_decode_returns(&provider.call(req).await.unwrap()).unwrap();

        let future_a_call = future_ACall {};
        let req = TransactionRequest {
            to: Some(TxKind::Call(tripool_address)),
            input: Some(Bytes::from(future_a_call.abi_encode())).into(),
            ..Default::default()
        };
        let future_a =
            future_ACall::abi_decode_returns(&provider.call(req).await.unwrap()).unwrap();

        let initial_a_time_call = initial_A_timeCall {};
        let req = TransactionRequest {
            to: Some(TxKind::Call(tripool_address)),
            input: Some(Bytes::from(initial_a_time_call.abi_encode())).into(),
            ..Default::default()
        };
        let initial_a_time =
            initial_A_timeCall::abi_decode_returns(&provider.call(req).await.unwrap()).unwrap();

        let future_a_time_call = future_A_timeCall {};
        let req = TransactionRequest {
            to: Some(TxKind::Call(tripool_address)),
            input: Some(Bytes::from(future_a_time_call.abi_encode())).into(),
            ..Default::default()
        };
        let future_a_time =
            future_A_timeCall::abi_decode_returns(&provider.call(req).await.unwrap()).unwrap();

        let initial_a_time_u64 = initial_a_time.to::<u64>();
        let future_a_time_u64 = future_a_time.to::<u64>();

        assert_eq!(
            a(
                initial_a_time_u64,
                initial_a,
                initial_a_time_u64,
                future_a,
                future_a_time_u64
            )
            .unwrap(),
            initial_a
        );
        assert_eq!(
            a(
                future_a_time_u64,
                initial_a,
                initial_a_time_u64,
                future_a,
                future_a_time_u64
            )
            .unwrap(),
            future_a
        );

        let midpoint_time = (initial_a_time_u64 + future_a_time_u64) / 2;
        let midpoint_a = a(
            midpoint_time,
            initial_a,
            initial_a_time_u64,
            future_a,
            future_a_time_u64,
        )
        .unwrap();

        let expected_midpoint_a = if future_a > initial_a {
            initial_a + (future_a - initial_a) / U256::from(2)
        } else {
            initial_a - (initial_a - future_a) / U256::from(2)
        };
        assert_eq!(midpoint_a, expected_midpoint_a);
    }
}
