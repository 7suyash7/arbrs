use alloy_primitives::Address;
use alloy_sol_types::{sol, SolEvent};
use crate::errors::ArbRsError;
use alloy_provider::Provider;
use alloy_rpc_types::{Filter, Log};
use std::sync::Arc;

// ABI definition for the Uniswap V2 Factory's `PairCreated` event
sol! {
    event PairCreated(
        address indexed token0,
        address indexed token1,
        address pair,
        uint256
    );
}

// ABI definition for the UniswapV3 `PoolCreated` event
sol! {
    event PoolCreated(
        address indexed token0,
        address indexed token1,
        uint24 indexed fee,
        int24 tickSpacing,
        address pool
    );
}

/// Represents the data from a discovered V2 pool
#[derive(Debug, Clone, Copy)]
pub struct DiscoveredV2Pool {
    pub token0: Address,
    pub token1: Address,
    pub pool_address: Address,
}

/// Represents the data from a discovered V3 pool
#[derive(Debug, Clone, Copy)]
pub struct DiscoveredV3Pool {
    pub token0: Address,
    pub token1: Address,
    pub fee: u32,
    pub tick_spacing: i32,
    pub pool_address: Address,
}

pub async fn discover_new_v2_pools<P: Provider + Send + Sync + 'static + ?Sized>(
    provider: Arc<P>,
    factory_address: Address,
    from_block: u64,
    to_block: u64,
) -> Result<Vec<DiscoveredV2Pool>, ArbRsError> {
    let event_filter = Filter::new()
        .address(factory_address)
        .event_signature(PairCreated::SIGNATURE_HASH)
        .from_block(from_block)
        .to_block(to_block);

    let logs: Vec<Log> = provider
        .get_logs(&event_filter)
        .await
        .map_err(|e| {
            ArbRsError::ProviderError(e.to_string())
        })?;

    let mut discovered_pools = Vec::new();

    for (i, log) in logs.iter().enumerate() {
        match PairCreated::decode_log(&log.inner) {
            Ok(decoded_log) => {
                discovered_pools.push(DiscoveredV2Pool {
                    token0: decoded_log.token0,
                    token1: decoded_log.token1,
                    pool_address: decoded_log.pair,
                });
            }
            Err(e) => {
                println!("[discover_new_v2_pools] FAILED to decode log #{}: {:?}", i + 1, e);
            }
        }
    }

    Ok(discovered_pools)
}

pub async fn discover_new_v3_pools<P: Provider + Send + Sync + 'static + ?Sized>(
    provider: Arc<P>,
    factory_address: Address,
    from_block: u64,
    to_block: u64,
) -> Result<Vec<DiscoveredV3Pool>, ArbRsError> {
    let event_filter = Filter::new()
        .address(factory_address)
        .event_signature(PoolCreated::SIGNATURE_HASH)
        .from_block(from_block)
        .to_block(to_block);

    let logs: Vec<Log> = provider
        .get_logs(&event_filter)
        .await
        .map_err(|e| ArbRsError::ProviderError(e.to_string()))?;

    let mut discovered_pools = Vec::new();
    for log in logs {
        let decoded_log = PoolCreated::decode_log(&log.inner)
            .map_err(|e| ArbRsError::AbiDecodeError(e.to_string()))?;
        discovered_pools.push(DiscoveredV3Pool {
            token0: decoded_log.token0,
            token1: decoded_log.token1,
            fee: decoded_log.fee.to(), 
            tick_spacing: decoded_log.tickSpacing.as_i32(),
            pool_address: decoded_log.pool,
        });
    }
    Ok(discovered_pools)
}
