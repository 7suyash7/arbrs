use alloy_primitives::{Address, U256, address};
use alloy_provider::{Provider, ProviderBuilder};
use arbrs::core::token::TokenLike;
use arbrs::manager::token_manager::TokenManager;
use std::sync::Arc;
use url::Url;

const WETH_ADDRESS: Address = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
const WBTC_ADDRESS: Address = address!("2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599");
const VITALIK_ADDRESS: Address = address!("d8dA6BF26964aF9D7eEd9e03E53415D37aA96045");
const ROUTER_ADDRESS: Address = address!("7a250d5630B4cF539739dF2C5dAcb4c659F2488D");
const ZERO_ADDRESS: Address = address!("0000000000000000000000000000000000000000");

const FORK_RPC_URL: &str = "http://127.0.0.1:8545";

type DynProvider = dyn Provider + Send + Sync;

async fn setup_manager() -> TokenManager<DynProvider> {
    let url = Url::parse(FORK_RPC_URL).expect("Failed to parse RPC URL");
    let provider = ProviderBuilder::new().connect_http(url);
    let provider_arc: Arc<DynProvider> = Arc::new(provider);
    TokenManager::new(provider_arc, 1)
}

#[tokio::test]
async fn test_fetch_known_token_properties() {
    let manager = setup_manager().await;
    let weth_token = manager.get_token(WETH_ADDRESS).await.unwrap();
    let wbtc_token = manager.get_token(WBTC_ADDRESS).await.unwrap();

    assert_eq!(weth_token.symbol(), "WETH");
    assert_eq!(weth_token.decimals(), 18);
    assert_eq!(wbtc_token.symbol(), "WBTC");
    assert_eq!(wbtc_token.decimals(), 8);
}

#[tokio::test]
async fn test_get_balance_and_total_supply() {
    let manager = setup_manager().await;
    let weth_token = manager.get_token(WETH_ADDRESS).await.unwrap();
    let balance_result = weth_token.get_balance(VITALIK_ADDRESS, None).await;
    let total_supply_result = weth_token.get_total_supply(None).await;

    assert!(
        balance_result.is_ok(),
        "Balance fetch failed: {:?}",
        balance_result.err()
    );
    assert!(
        total_supply_result.is_ok(),
        "Total supply fetch failed: {:?}",
        total_supply_result.err()
    );
    assert!(
        total_supply_result.unwrap() > U256::ZERO,
        "Total supply should be greater than zero"
    );
}

#[tokio::test]
async fn test_get_allowance() {
    let manager = setup_manager().await;
    let weth_token = manager.get_token(WETH_ADDRESS).await.unwrap();
    let allowance_result = weth_token
        .get_allowance(VITALIK_ADDRESS, ROUTER_ADDRESS, None)
        .await;
    assert!(
        allowance_result.is_ok(),
        "Allowance fetch failed: {:?}",
        allowance_result.err()
    );
}

#[tokio::test]
async fn test_non_compliant_token_fallback() {
    let manager = setup_manager().await;
    let mkr_address = address!("9f8F72aA9304c8B593d555F12eF6589cC3A579A2");
    let mkr_token = manager.get_token(mkr_address).await.unwrap();

    assert_eq!(mkr_token.symbol(), "MKR");
    assert_eq!(mkr_token.decimals(), 18);
}

#[tokio::test]
async fn test_native_ether_placeholder() {
    let manager = setup_manager().await;
    let eth_token = manager.get_token(ZERO_ADDRESS).await.unwrap();

    assert_eq!(eth_token.symbol(), "ETH");
    assert_eq!(eth_token.decimals(), 18);

    let allowance = eth_token
        .get_allowance(VITALIK_ADDRESS, ROUTER_ADDRESS, None)
        .await
        .unwrap();
    assert_eq!(allowance, U256::MAX);

    let balance = eth_token.get_balance(VITALIK_ADDRESS, None).await.unwrap();
    assert!(balance > U256::ZERO);
}

#[tokio::test]
async fn test_token_comparisons() {
    let manager = setup_manager().await;
    let weth_token = manager.get_token(WETH_ADDRESS).await.unwrap();
    let wbtc_token = manager.get_token(WBTC_ADDRESS).await.unwrap();

    assert_eq!(*weth_token, WETH_ADDRESS);
    assert_eq!(*wbtc_token, WBTC_ADDRESS);
    assert_ne!(*weth_token, *wbtc_token);

    assert!(*weth_token > *wbtc_token);
    assert!(*wbtc_token < *weth_token);
}
