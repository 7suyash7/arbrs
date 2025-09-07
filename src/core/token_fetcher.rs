use crate::core::token::Erc20Data;
use crate::errors::ArbRsError;
use alloy_primitives::{Address, B256, Bytes, TxKind};
use alloy_provider::Provider;
use alloy_rpc_types::TransactionRequest;
use alloy_sol_types::{sol, SolCall};
use std::sync::Arc;

// ABI defs
sol!(
    function symbol() external view returns (string memory);
    function symbol_bytes32() external view returns (bytes32);
    function decimals() external view returns (uint8);
    function name() external view returns (string memory);
    function name_bytes32() external view returns (bytes32);
);

pub struct TokenFetcher<P: ?Sized> {
    provider: Arc<P>,
}

impl<P: Provider + Send + Sync + 'static + ?Sized> TokenFetcher<P> {
    pub fn new(provider: Arc<P>) -> Self {
        Self { provider }
    }

    pub async fn fetch_erc20_data(&self, address: Address) -> Result<Erc20Data<P>, ArbRsError> {
        let (decimals_res, symbol_res, name_res) = tokio::join!(
            self.fetch_decimals(address),
            self.fetch_symbol(address),
            self.fetch_name(address)
        );

        let decimals = decimals_res?;
        let symbol = symbol_res.unwrap_or_else(|| format!("UNKNOWN@{}", address_to_short_string(address)));
        let name = name_res.unwrap_or_else(|| "Unknown Token".to_string());

        Ok(Erc20Data::new(
            address,
            symbol,
            name,
            decimals,
            Arc::clone(&self.provider),
        ))
    }

    async fn fetch_decimals(&self, address: Address) -> Result<u8, ArbRsError> {
        let call = decimalsCall {};
        let request = TransactionRequest {
            to: Some(TxKind::Call(address)),
            input: Some(Bytes::from(call.abi_encode())).into(),
            ..Default::default()
        };
        match self.provider.call(request).await {
            Ok(result_bytes) => Ok(decimalsCall::abi_decode_returns(&result_bytes)
                .map_err(|e| ArbRsError::AbiDecodeError(e.to_string()))?),
            Err(e) => Err(ArbRsError::ProviderError(e.to_string())),
        }
    }

    /// Fetches symbol using a multi-step fallback process.
    async fn fetch_symbol(&self, address: Address) -> Option<String> {
        println!("[{address}] Fetching symbol...");
        let calldata = symbolCall {}.abi_encode();
        let request = TransactionRequest {
            to: Some(TxKind::Call(address)),
            input: Some(calldata.into()).into(),
            ..Default::default()
        };

        match self.provider.call(request).await {
            Ok(result_bytes) => {
                println!("[{address}] Call successful. Trying decoders...");
                if let Ok(decoded_string) = symbolCall::abi_decode_returns(&result_bytes) {
                    let symbol = decoded_string.trim().to_string();
                    if !symbol.is_empty() && symbol.chars().any(|c| c.is_alphanumeric()) {
                        println!("[{address}] Decoded as string: \"{symbol}\"");
                        return Some(symbol);
                    }
                }

                if let Ok(decoded_bytes) = symbol_bytes32Call::abi_decode_returns(&result_bytes) {
                    let symbol = bytes32_to_string(&decoded_bytes);
                    if !symbol.is_empty() {
                         println!("[{address}] Decoded as bytes32: \"{symbol}\"");
                        return Some(symbol);
                    }
                }
                println!("[{address}] Decoding failed for both string and bytes32.");
                None
            }
            Err(e) => {
                println!("[{address}] Call reverted or failed: {e}");
                None
            }
        }
    }

    /// Fetches name using a multi-step fallback process.
    async fn fetch_name(&self, address: Address) -> Option<String> {
        println!("[{address}] Fetching name...");
        let calldata = nameCall {}.abi_encode();
        let request = TransactionRequest {
            to: Some(TxKind::Call(address)),
            input: Some(calldata.into()).into(),
            ..Default::default()
        };

        match self.provider.call(request).await {
            Ok(result_bytes) => {
                println!("[{address}] Call successful. Trying decoders...");
                if let Ok(decoded_string) = nameCall::abi_decode_returns(&result_bytes) {
                    let name = decoded_string.trim().to_string();
                    if !name.is_empty() && name.chars().any(|c| c.is_alphanumeric()) {
                        println!("[{address}] Decoded as string: \"{name}\"");
                        return Some(name);
                    }
                }

                if let Ok(decoded_bytes) = name_bytes32Call::abi_decode_returns(&result_bytes) {
                    let name = bytes32_to_string(&decoded_bytes);
                    if !name.is_empty() {
                         println!("[{address}] Decoded as bytes32: \"{name}\"");
                        return Some(name);
                    }
                }
                println!("[{address}] Decoding failed for both string and bytes32.");
                None
            }
            Err(e) => {
                println!("[{address}] Call reverted or failed: {e}");
                None
            }
        }
    }
}

// Helper fns
fn bytes32_to_string(bytes: &B256) -> String {
    let first_null = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..first_null]).to_string()
}

fn address_to_short_string(address: Address) -> String {
    let hex = address.to_string();
    format!("0x{}..{}", &hex[2..6], &hex[hex.len() - 4..])
}