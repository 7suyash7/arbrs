use crate::{ArbRsError, pool::uniswap_v3::TickInfo};
use alloy_primitives::{Address, U256};
use alloy_provider::Provider;
use alloy_rpc_types::{Filter, Log as RpcLog};
use alloy_sol_types::{SolEvent, sol};
use std::collections::BTreeMap;
use std::sync::Arc;

sol! {
    event Mint(address sender, address indexed owner, int24 indexed tickLower, int24 indexed tickUpper, uint128 amount, uint256 amount0, uint256 amount1);
    event Burn(address indexed owner, int24 indexed tickLower, int24 indexed tickUpper, uint128 amount, uint256 amount0, uint256 amount1);
}

/// Represents a raw liquidity change event fetched from the blockchain.
#[derive(Debug, Clone)]
pub struct UniswapV3LiquidityEvent {
    pub block_number: u64,
    pub tx_index: u64,
    pub log_index: u64,
    pub liquidity: i128,
    pub tick_lower: i32,
    pub tick_upper: i32,
}

/// A processed update ready to be applied to a pool's liquidity map.
#[derive(Debug, Clone)]
pub struct UniswapV3PoolLiquidityMappingUpdate {
    pub block_number: u64,
    pub liquidity: i128,
    pub tick_lower: i32,
    pub tick_upper: i32,
}

/// A complete snapshot of a pool's tick-level liquidity.
#[derive(Debug, Clone, Default)]
pub struct LiquidityMap {
    pub tick_bitmap: BTreeMap<i16, U256>,
    pub tick_data: BTreeMap<i32, TickInfo>,
}

pub struct UniswapV3LiquiditySnapshot<P: ?Sized> {
    provider: Arc<P>,
    chain_id: u64,
    newest_block: u64,
    pub liquidity_events: BTreeMap<Address, Vec<UniswapV3LiquidityEvent>>,
    pub liquidity_snapshot: BTreeMap<Address, LiquidityMap>,
}

impl<P: Provider + Send + Sync + 'static + ?Sized> UniswapV3LiquiditySnapshot<P> {
    pub fn new(provider: Arc<P>, chain_id: u64, start_block: u64) -> Self {
        Self {
            provider,
            chain_id,
            newest_block: start_block,
            liquidity_events: BTreeMap::new(),
            liquidity_snapshot: BTreeMap::new(),
        }
    }

    /// Fetches and processes new Mint and Burn events up to a specified block.
    pub async fn fetch_new_events(&mut self, to_block: u64) -> Result<(), ArbRsError> {
        if to_block <= self.newest_block {
            return Ok(());
        }

        println!(
            "Updating Uniswap V3 snapshot from block {} to {}",
            self.newest_block, to_block
        );

        let mint_filter = Filter::new()
            .from_block(self.newest_block + 1)
            .to_block(to_block)
            .event_signature(Mint::SIGNATURE_HASH);

        let burn_filter = Filter::new()
            .from_block(self.newest_block + 1)
            .to_block(to_block)
            .event_signature(Burn::SIGNATURE_HASH);

        let (mint_logs_res, burn_logs_res) = tokio::join!(
            self.provider.get_logs(&mint_filter),
            self.provider.get_logs(&burn_filter)
        );

        let mint_logs = mint_logs_res.map_err(|e| ArbRsError::ProviderError(e.to_string()))?;
        let burn_logs = burn_logs_res.map_err(|e| ArbRsError::ProviderError(e.to_string()))?;

        let all_logs = mint_logs.into_iter().chain(burn_logs.into_iter());

        for log in all_logs {
            let (pool_address, event) = self.process_log(&log)?;
            self.liquidity_events
                .entry(pool_address)
                .or_default()
                .push(event);
        }

        self.newest_block = to_block;
        Ok(())
    }

    fn process_log(&self, log: &RpcLog) -> Result<(Address, UniswapV3LiquidityEvent), ArbRsError> {
        let pool_address = log.address();
        let topics = log.topics();

        let (liquidity, tick_lower, tick_upper) = if topics[0] == Mint::SIGNATURE_HASH {
            let decoded = Mint::decode_log_data(&log.inner.data)?;
            (decoded.amount as i128, decoded.tickLower, decoded.tickUpper)
        } else if topics[0] == Burn::SIGNATURE_HASH {
            let decoded = Burn::decode_log_data(&log.inner.data)?;
            (
                -(decoded.amount as i128),
                decoded.tickLower,
                decoded.tickUpper,
            )
        } else {
            return Err(ArbRsError::AbiDecodeError(
                "Unknown event signature".to_string(),
            ));
        };

        Ok((
            pool_address,
            UniswapV3LiquidityEvent {
                block_number: log.block_number.unwrap_or(0),
                tx_index: log.transaction_index.unwrap_or(0),
                log_index: log.log_index.unwrap_or(0),
                liquidity,
                tick_lower: tick_lower.try_into().unwrap(),
                tick_upper: tick_upper.try_into().unwrap(),
            },
        ))
    }

    /// Consumes pending liquidity updates for a pool, sorted chronologically.
    pub fn pending_updates(
        &mut self,
        pool_address: Address,
    ) -> Vec<UniswapV3PoolLiquidityMappingUpdate> {
        if let Some(events) = self.liquidity_events.remove(&pool_address) {
            let mut sorted_events = events;
            sorted_events.sort_by_key(|e| (e.block_number, e.tx_index, e.log_index));

            sorted_events
                .into_iter()
                .map(|event| UniswapV3PoolLiquidityMappingUpdate {
                    block_number: event.block_number,
                    liquidity: event.liquidity,
                    tick_lower: event.tick_lower,
                    tick_upper: event.tick_upper,
                })
                .collect()
        } else {
            Vec::new()
        }
    }
}
