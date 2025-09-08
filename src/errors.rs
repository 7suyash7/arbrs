use alloy_primitives::Address;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ArbRsError {
    #[error("Provider error: {0}")]
    ProviderError(String),

    #[error("ABI decoding error for contract call: {0}")]
    AbiDecodeError(String),

    #[error("Token implementation non-standard at address {0}: {1}")]
    TokenStandardError(Address, String),

    #[error("Could not fetch required data for address: {0}")]
    DataFetchError(Address),

    #[error("Pool calculation error: {0}")]
    CalculationError(String),
}
