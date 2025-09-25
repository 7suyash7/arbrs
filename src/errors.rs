use alloy::transports::{RpcError, TransportErrorKind};
use alloy_primitives::Address;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
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

    #[error("Uniswap V3 Math Error: {0}")]
    UniswapV3MathError(String),

    #[error("No pool state known prior to block {0}")]
    NoPoolStateAvailable(u64),

    #[error(
        "Update attempted for a block ({attempted_block}) prior to the last recorded update ({latest_block})"
    )]
    LateUpdateError {
        attempted_block: u64,
        latest_block: u64,
    },

    #[error("ABI Decode Error: {0}")]
    SolAbiError(#[from] alloy_sol_types::Error),

    #[error("This pool is known to be broken and is not supported.")]
    BrokenPool,
}

impl From<RpcError<TransportErrorKind>> for ArbRsError {
    fn from(error: RpcError<TransportErrorKind>) -> Self {
        ArbRsError::ProviderError(error.to_string())
    }
}
