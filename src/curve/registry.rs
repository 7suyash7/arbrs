use crate::errors::ArbRsError;
use alloy_primitives::{Address, U256};
use alloy_provider::Provider;
use alloy_rpc_types::TransactionRequest;
use alloy_sol_types::{SolCall, sol};
use std::sync::Arc;

sol! {
    // Interface for the main Curve registry
    interface ICurveRegistry {
        function get_lp_token(address pool) external view returns (address);
        function get_pool_from_lp_token(address lp_token) external view returns (address);
        function get_underlying_coins(address pool) external view returns (address[8]);
    }

    // Interface for a generic Curve pool to get its coins
    interface ICurvePool {
        function coins(uint256 i) external view returns (address);
    }
}

#[derive(Clone)]
pub struct CurveRegistry<P: Provider + Send + Sync + 'static + ?Sized> {
    pub address: Address,
    provider: Arc<P>,
}

impl<P: Provider + Send + Sync + 'static + ?Sized> CurveRegistry<P> {
    pub fn new(address: Address, provider: Arc<P>) -> Self {
        Self { address, provider }
    }

    pub async fn get_lp_token(&self, pool_address: Address) -> Result<Address, ArbRsError> {
        let call = ICurveRegistry::get_lp_tokenCall { pool: pool_address };
        let request = TransactionRequest::default()
            .to(self.address)
            .input(call.abi_encode().into());
        let result_bytes = self
            .provider
            .call(request)
            .await
            .map_err(|e| ArbRsError::ProviderError(e.to_string()))?;
        let decoded = ICurveRegistry::get_lp_tokenCall::abi_decode_returns(&result_bytes)?;
        Ok(decoded)
    }

    pub async fn get_underlying_coins(
        &self,
        pool_address: Address,
    ) -> Result<Vec<Address>, ArbRsError> {
        let call = ICurveRegistry::get_underlying_coinsCall { pool: pool_address };
        let request = TransactionRequest::default()
            .to(self.address)
            .input(call.abi_encode().into());
        let result_bytes = self
            .provider
            .call(request)
            .await
            .map_err(|e| ArbRsError::ProviderError(e.to_string()))?;
        let decoded = ICurveRegistry::get_underlying_coinsCall::abi_decode_returns(&result_bytes)?;
        Ok(decoded
            .into_iter()
            .filter(|&addr| !addr.is_zero())
            .collect())
    }

    /// Finds the base pool for a given metapool. Returns `Ok(None)` if it's not a metapool.
    pub async fn get_base_pool(
        &self,
        metapool_address: Address,
    ) -> Result<Option<Address>, ArbRsError> {
        println!(
            "[get_base_pool] Checking for base pool for {}",
            metapool_address
        );
        let get_coin_call = ICurvePool::coinsCall { i: U256::from(1) };
        let request = TransactionRequest::default()
            .to(metapool_address)
            .input(get_coin_call.abi_encode().into());

        let base_lp_token = match self.provider.call(request).await {
            Ok(bytes) => ICurvePool::coinsCall::abi_decode_returns(&bytes)?,
            Err(e) => {
                println!(
                    "[get_base_pool] Call to coins(1) failed: {}. Assuming not a metapool.",
                    e
                );
                return Ok(None);
            }
        };

        if base_lp_token.is_zero() {
            return Ok(None);
        }

        let get_pool_call = ICurveRegistry::get_pool_from_lp_tokenCall {
            lp_token: base_lp_token,
        };
        let request = TransactionRequest::default()
            .to(self.address)
            .input(get_pool_call.abi_encode().into());

        match self.provider.call(request).await {
            Ok(result_bytes) => {
                let base_pool_address =
                    ICurveRegistry::get_pool_from_lp_tokenCall::abi_decode_returns(&result_bytes)?;

                if base_pool_address.is_zero() {
                    Ok(None)
                } else {
                    Ok(Some(base_pool_address))
                }
            }
            Err(_) => Ok(None),
        }
    }
}
