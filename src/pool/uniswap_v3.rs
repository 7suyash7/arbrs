use crate::TokenLike;
use crate::core::token::Token;
use crate::errors::ArbRsError;
use crate::math::v3::tick_bitmap::position;
use crate::math::v3::{
    constants::{MAX_SQRT_RATIO, MAX_TICK, MIN_SQRT_RATIO, MIN_TICK},
    liquidity_math, swap_math, tick_bitmap,
    tick_math::{self},
};
use crate::pool::LiquidityPool;
use crate::pool::uniswap_v3_snapshot::{LiquidityMap, UniswapV3PoolLiquidityMappingUpdate};
use alloy_primitives::{Address, Bytes, I256, U256};
use alloy_provider::Provider;
use alloy_rpc_types::{BlockId, TransactionRequest};
use alloy_sol_types::{SolCall, sol};
use async_trait::async_trait;
use std::any::Any;
use std::collections::BTreeMap;
use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::sync::Arc;
use tokio::sync::RwLock;

// ABI Definition for slot0 and liquidity
sol! {
    function slot0() external view returns (uint160 sqrtPriceX96, int24 tick, uint16 observationIndex, uint16 observationCardinality, uint16 observationCardinalityNext, uint8 feeProtocol, bool unlocked);
    function liquidity() external view returns (uint128);
    function tickBitmap(int16 wordPosition) external view returns (uint256);
    function ticks(int24 tick) external view returns (uint128 liquidityGross, int128 liquidityNet, uint256 feeGrowthOutside0X128, uint256 feeGrowthOutside1X128, int56 tickCumulativeOutside, uint160 secondsPerLiquidityOutsideX128, uint32 secondsOutside, bool initialized);
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TickInfo {
    pub liquidity_gross: u128,
    pub liquidity_net: i128,
    // other fields can be added later if needed for fee calculations, etc.
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct UniswapV3PoolState {
    pub liquidity: u128,
    pub sqrt_price_x96: U256,
    pub tick: i32,
    pub block_number: u64,
    pub tick_bitmap: BTreeMap<i16, U256>,
    pub tick_data: BTreeMap<i32, TickInfo>,
}

/// Represents the state of a swap calculation as it progresses
struct SwapState {
    amount_specified_remaining: I256,
    amount_calculated: I256,
    sqrt_price_x96: U256,
    tick: i32,
    liquidity: u128,
}

/// Holds the results of a V3 pool simulation.
#[derive(Debug, Clone)]
pub struct UniswapV3PoolSimulationResult {
    pub amount0_delta: I256,
    pub amount1_delta: I256,
    pub initial_state: UniswapV3PoolState,
    pub final_state: UniswapV3PoolState,
}

pub struct UniswapV3Pool<P: ?Sized> {
    address: Address,
    token0: Arc<Token<P>>,
    token1: Arc<Token<P>>,
    fee: u32,
    tick_spacing: i32,
    pub state: RwLock<UniswapV3PoolState>,
    provider: Arc<P>,
    state_cache: RwLock<BTreeMap<u64, UniswapV3PoolState>>,
    _min_word: i16,
    _max_word: i16,
}

impl<P: Provider + Send + Sync + 'static + ?Sized> UniswapV3Pool<P> {
    pub fn new(
        address: Address,
        token0: Arc<Token<P>>,
        token1: Arc<Token<P>>,
        fee: u32,
        tick_spacing: i32,
        provider: Arc<P>,
        initial_liquidity_map: Option<LiquidityMap>,
    ) -> Self {
        let (tick_bitmap, tick_data) = match initial_liquidity_map {
            Some(map) => (map.tick_bitmap, map.tick_data),
            None => (BTreeMap::new(), BTreeMap::new()),
        };

        let (min_word, _) = position(MIN_TICK / tick_spacing);
        let (max_word, _) = position(MAX_TICK / tick_spacing);

        Self {
            address,
            token0,
            token1,
            fee,
            tick_spacing,
            state: RwLock::new(UniswapV3PoolState {
                tick_bitmap,
                tick_data,
                ..Default::default()
            }),
            provider,
            state_cache: RwLock::new(BTreeMap::new()),
            _min_word: min_word,
            _max_word: max_word,
        }
    }

    /// Applies an update to the liquidity map.
    pub async fn update_liquidity_map(&self, update: UniswapV3PoolLiquidityMappingUpdate) {
        let mut state = self.state.write().await;

        if update.tick_lower <= state.tick && state.tick < update.tick_upper {
            state.liquidity =
                liquidity_math::add_delta(state.liquidity, update.liquidity).unwrap_or(0);
        }

        let lower_tick_info = state.tick_data.entry(update.tick_lower).or_default();
        lower_tick_info.liquidity_gross =
            (lower_tick_info.liquidity_gross as i128 + update.liquidity) as u128;
        lower_tick_info.liquidity_net += update.liquidity;

        let upper_tick_info = state.tick_data.entry(update.tick_upper).or_default();
        upper_tick_info.liquidity_gross =
            (upper_tick_info.liquidity_gross as i128 - update.liquidity) as u128;
        upper_tick_info.liquidity_net -= update.liquidity;
    }

    /// Internal swap calculation logic
    async fn _calculate_swap(
        &self,
        zero_for_one: bool,
        amount_specified: I256,
        sqrt_price_limit_x96: U256,
        override_state: Option<&UniswapV3PoolState>,
    ) -> Result<(I256, I256, UniswapV3PoolState), ArbRsError> {
        let state_guard = self.state.read().await;
        let initial_state = override_state.unwrap_or(&state_guard);

        if amount_specified.is_zero() {
            return Err(ArbRsError::CalculationError(
                "Amount specified cannot be zero".into(),
            ));
        }

        let exact_input = amount_specified.is_positive();
        let mut current_state = initial_state.clone();

        let mut swap_state = SwapState {
            amount_specified_remaining: amount_specified,
            amount_calculated: I256::ZERO,
            sqrt_price_x96: current_state.sqrt_price_x96,
            tick: current_state.tick,
            liquidity: current_state.liquidity,
        };

        while !swap_state.amount_specified_remaining.is_zero()
            && swap_state.sqrt_price_x96 != sqrt_price_limit_x96
        {
            let (mut word_pos, _) = tick_bitmap::position(swap_state.tick / self.tick_spacing);

            let (next_tick, initialized) = {
                let mut result = None;

                if !current_state.tick_bitmap.contains_key(&word_pos) {
                    self._fetch_and_populate_initialized_ticks(
                        word_pos,
                        &mut current_state.tick_bitmap,
                        &mut current_state.tick_data,
                    )
                    .await?;
                }
                let bitmap = current_state
                    .tick_bitmap
                    .get(&word_pos)
                    .copied()
                    .unwrap_or_default();

                if let Some(found_tick) = tick_bitmap::next_initialized_tick_within_one_word(
                    bitmap,
                    swap_state.tick,
                    self.tick_spacing,
                    zero_for_one,
                ) {
                    result = Some(found_tick);
                } else {
                    if zero_for_one {
                        word_pos -= 1;
                        while word_pos >= self._min_word {
                            if !current_state.tick_bitmap.contains_key(&word_pos) {
                                self._fetch_and_populate_initialized_ticks(
                                    word_pos,
                                    &mut current_state.tick_bitmap,
                                    &mut current_state.tick_data,
                                )
                                .await?;
                            }
                            let next_bitmap = current_state
                                .tick_bitmap
                                .get(&word_pos)
                                .copied()
                                .unwrap_or_default();
                            if next_bitmap != U256::ZERO {
                                let next_initialized_tick = (word_pos as i32 * 256
                                    + crate::math::v3::bit_math::most_significant_bit(next_bitmap)
                                        as i32)
                                    * self.tick_spacing;
                                result = Some((next_initialized_tick, true));
                                break;
                            }
                            word_pos -= 1;
                        }
                    } else {
                        word_pos += 1;
                        while word_pos <= self._max_word {
                            if !current_state.tick_bitmap.contains_key(&word_pos) {
                                self._fetch_and_populate_initialized_ticks(
                                    word_pos,
                                    &mut current_state.tick_bitmap,
                                    &mut current_state.tick_data,
                                )
                                .await?;
                            }
                            let next_bitmap = current_state
                                .tick_bitmap
                                .get(&word_pos)
                                .copied()
                                .unwrap_or_default();
                            if next_bitmap != U256::ZERO {
                                let next_initialized_tick = (word_pos as i32 * 256
                                    + crate::math::v3::bit_math::least_significant_bit(next_bitmap)
                                        as i32)
                                    * self.tick_spacing;
                                result = Some((next_initialized_tick, true));
                                break;
                            }
                            word_pos += 1;
                        }
                    }
                }
                result.unwrap_or((if zero_for_one { MIN_TICK } else { MAX_TICK }, false))
            };

            let next_tick = next_tick.clamp(MIN_TICK, MAX_TICK);

            let sqrt_price_next_tick = tick_math::get_sqrt_ratio_at_tick(next_tick)?;

            let sqrt_price_target = if (zero_for_one && sqrt_price_next_tick < sqrt_price_limit_x96)
                || (!zero_for_one && sqrt_price_next_tick > sqrt_price_limit_x96)
            {
                sqrt_price_limit_x96
            } else {
                sqrt_price_next_tick
            };

            let step = swap_math::compute_swap_step(
                swap_state.sqrt_price_x96,
                sqrt_price_target,
                swap_state.liquidity,
                swap_state.amount_specified_remaining,
                self.fee,
            )?;

            swap_state.sqrt_price_x96 = step.sqrt_ratio_next_x96;

            if exact_input {
                swap_state.amount_specified_remaining -= I256::from_raw(step.amount_in);
                swap_state.amount_calculated -= I256::from_raw(step.amount_out);
            } else {
                swap_state.amount_specified_remaining += I256::from_raw(step.amount_out);
                swap_state.amount_calculated += I256::from_raw(step.amount_in);
            }

            if swap_state.sqrt_price_x96 == sqrt_price_next_tick {
                if initialized {
                    let liquidity_net = current_state
                        .tick_data
                        .get(&next_tick)
                        .map(|t| t.liquidity_net)
                        .unwrap_or(0);
                    swap_state.liquidity = liquidity_math::add_delta(
                        swap_state.liquidity,
                        if zero_for_one {
                            -liquidity_net
                        } else {
                            liquidity_net
                        },
                    )
                    .ok_or(ArbRsError::CalculationError(
                        "Liquidity underflow/overflow".into(),
                    ))?;
                }
                swap_state.tick = if zero_for_one {
                    next_tick - 1
                } else {
                    next_tick
                };
            } else {
                swap_state.tick = tick_math::get_tick_at_sqrt_ratio(swap_state.sqrt_price_x96)?;
            }
        }

        let (amount0_delta, amount1_delta) = if zero_for_one {
            (
                amount_specified - swap_state.amount_specified_remaining,
                swap_state.amount_calculated,
            )
        } else {
            (
                swap_state.amount_calculated,
                amount_specified - swap_state.amount_specified_remaining,
            )
        };

        let final_state = UniswapV3PoolState {
            liquidity: swap_state.liquidity,
            sqrt_price_x96: swap_state.sqrt_price_x96,
            tick: swap_state.tick,
            ..initial_state.clone()
        };

        Ok((amount0_delta, amount1_delta, final_state))
    }

    /// Fetches state at a specific block number without updating the live state.
    async fn _fetch_state_at_block(
        &self,
        block_number: u64,
    ) -> Result<UniswapV3PoolState, ArbRsError> {
        let block_id = BlockId::from(block_number);

        let slot0_call = slot0Call {};
        let slot0_request = TransactionRequest {
            to: Some(self.address.into()),
            input: Some(Bytes::from(slot0_call.abi_encode())).into(),
            ..Default::default()
        };

        let liquidity_call = liquidityCall {};
        let liquidity_request = TransactionRequest {
            to: Some(self.address.into()),
            input: Some(Bytes::from(liquidity_call.abi_encode())).into(),
            ..Default::default()
        };

        let (slot0_res, liquidity_res) = tokio::join!(
            self.provider.call(slot0_request).block(block_id),
            self.provider.call(liquidity_request).block(block_id)
        );

        let slot0_bytes = slot0_res.map_err(|e| ArbRsError::ProviderError(e.to_string()))?;
        let liquidity_bytes =
            liquidity_res.map_err(|e| ArbRsError::ProviderError(e.to_string()))?;

        let slot0_decoded = slot0Call::abi_decode_returns(&slot0_bytes)
            .map_err(|e| ArbRsError::AbiDecodeError(e.to_string()))?;
        let liquidity_decoded = liquidityCall::abi_decode_returns(&liquidity_bytes)
            .map_err(|e| ArbRsError::AbiDecodeError(e.to_string()))?;

        Ok(UniswapV3PoolState {
            sqrt_price_x96: U256::from(slot0_decoded.sqrtPriceX96),
            tick: slot0_decoded.tick.as_i32(),
            liquidity: liquidity_decoded,
            block_number,
            tick_bitmap: BTreeMap::new(),
            tick_data: BTreeMap::new(),
        })
    }

    async fn _fetch_and_populate_initialized_ticks(
        &self,
        word_pos: i16,
        tick_bitmap: &mut BTreeMap<i16, U256>,
        tick_data: &mut BTreeMap<i32, TickInfo>,
    ) -> Result<(), ArbRsError> {
        println!("Fetching on-demand tick data for word_pos: {}", word_pos);

        let bitmap_call = tickBitmapCall {
            wordPosition: word_pos,
        };
        let request = TransactionRequest {
            to: Some(self.address.into()),
            input: Some(Bytes::from(bitmap_call.abi_encode())).into(),
            ..Default::default()
        };

        let bitmap_bytes = self
            .provider
            .call(request.clone())
            .await
            .map_err(|e| ArbRsError::ProviderError(e.to_string()))?;
        let bitmap_word = tickBitmapCall::abi_decode_returns(&bitmap_bytes)?;

        tick_bitmap.insert(word_pos, bitmap_word);

        for i in 0..256 {
            if (bitmap_word >> i) & U256::from(1) != U256::ZERO {
                let compressed_tick = ((word_pos as i32) << 8) + i;

                let actual_tick = compressed_tick * self.tick_spacing;

                let ticks_call = ticksCall {
                    tick: actual_tick.try_into().map_err(|_| {
                        ArbRsError::CalculationError("Tick number out of bounds".to_string())
                    })?,
                };
                let request = TransactionRequest {
                    to: Some(self.address.into()),
                    input: Some(Bytes::from(ticks_call.abi_encode())).into(),
                    ..Default::default()
                };

                let tick_data_bytes = self
                    .provider
                    .call(request)
                    .await
                    .map_err(|e| ArbRsError::ProviderError(e.to_string()))?;
                let tick_decoded = ticksCall::abi_decode_returns(&tick_data_bytes)?;

                tick_data.insert(
                    actual_tick,
                    TickInfo {
                        liquidity_gross: tick_decoded.liquidityGross,
                        liquidity_net: tick_decoded.liquidityNet,
                    },
                );
            }
        }
        Ok(())
    }

    pub async fn simulate_exact_input_swap(
        &self,
        token_in: &Token<P>,
        amount_in: U256,
        override_state: Option<&UniswapV3PoolState>,
    ) -> Result<UniswapV3PoolSimulationResult, ArbRsError> {
        let zero_for_one = token_in.address() == self.token0.address();
        let amount_specified = I256::from_raw(amount_in);

        let sqrt_price_limit_x96 = if zero_for_one {
            MIN_SQRT_RATIO + U256::from(1)
        } else {
            MAX_SQRT_RATIO - U256::from(1)
        };

        let state_guard = self.state.read().await;
        let initial_state = override_state.unwrap_or(&state_guard);

        let (amount0_delta, amount1_delta, final_state) = self
            ._calculate_swap(
                zero_for_one,
                amount_specified,
                sqrt_price_limit_x96,
                Some(initial_state),
            )
            .await?;

        Ok(UniswapV3PoolSimulationResult {
            amount0_delta,
            amount1_delta,
            initial_state: initial_state.clone(),
            final_state,
        })
    }

    pub async fn simulate_exact_output_swap(
        &self,
        token_out: &Token<P>,
        amount_out: U256,
        override_state: Option<&UniswapV3PoolState>,
    ) -> Result<UniswapV3PoolSimulationResult, ArbRsError> {
        let zero_for_one = token_out.address() == self.token1.address();
        let amount_specified = I256::from_raw(amount_out);

        let sqrt_price_limit_x96 = if zero_for_one {
            MIN_SQRT_RATIO + U256::from(1)
        } else {
            MAX_SQRT_RATIO - U256::from(1)
        };

        let state_guard = self.state.read().await;
        let initial_state = override_state.unwrap_or(&state_guard);

        let (amount0_delta, amount1_delta, final_state) = self
            ._calculate_swap(
                zero_for_one,
                amount_specified,
                sqrt_price_limit_x96,
                Some(initial_state),
            )
            .await?;

        Ok(UniswapV3PoolSimulationResult {
            amount0_delta,
            amount1_delta,
            initial_state: initial_state.clone(),
            final_state,
        })
    }

    pub fn fee(&self) -> u32 {
        self.fee
    }

    pub fn tick_spacing(&self) -> i32 {
        self.tick_spacing
    }
}

#[async_trait]
impl<P: Provider + Send + Sync + 'static + ?Sized> LiquidityPool<P> for UniswapV3Pool<P> {
    fn address(&self) -> Address {
        self.address
    }

    fn tokens(&self) -> (Arc<Token<P>>, Arc<Token<P>>) {
        (self.token0.clone(), self.token1.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    async fn update_state(&self) -> Result<(), ArbRsError> {
        let latest_block = self
            .provider
            .get_block_number()
            .await
            .map_err(|e| ArbRsError::ProviderError(e.to_string()))?;

        let current_block_number = self.state.read().await.block_number;

        if latest_block < current_block_number {
            return Err(ArbRsError::LateUpdateError {
                attempted_block: latest_block,
                latest_block: current_block_number,
            });
        }

        if latest_block == current_block_number && current_block_number != 0 {
            return Ok(());
        }

        let new_state = self._fetch_state_at_block(latest_block).await?;

        let state_updated = {
            let state = self.state.read().await;
            state.sqrt_price_x96 != new_state.sqrt_price_x96
                || state.liquidity != new_state.liquidity
        };

        if state_updated {
            let mut state_writer = self.state.write().await;
            *state_writer = new_state.clone();

            let mut cache = self.state_cache.write().await;
            cache.insert(latest_block, new_state.clone());

            // pub sub logic here later
            // self.notify_subscribers(PublisherMessage::PoolStateUpdate(new_state)).await;
        }

        Ok(())
    }

    async fn calculate_tokens_out(
        &self,
        token_in: &Token<P>,
        amount_in: U256,
    ) -> Result<U256, ArbRsError> {
        let zero_for_one = token_in.address() == self.token0.address();
        let amount_specified = I256::from_raw(amount_in);

        let sqrt_price_limit_x96 = if zero_for_one {
            MIN_SQRT_RATIO + U256::from(1)
        } else {
            MAX_SQRT_RATIO - U256::from(1)
        };

        let (amount0_delta, amount1_delta, _final_state) = self
            ._calculate_swap(zero_for_one, amount_specified, sqrt_price_limit_x96, None)
            .await?;

        Ok(if zero_for_one {
            (-amount1_delta).into_raw()
        } else {
            (-amount0_delta).into_raw()
        })
    }

    async fn calculate_tokens_in_from_tokens_out(
        &self,
        token_out: &Token<P>,
        amount_out: U256,
    ) -> Result<U256, ArbRsError> {
        let zero_for_one = token_out.address() == self.token1.address();
        let amount_specified = I256::from_raw(amount_out);

        let sqrt_price_limit_x96 = if zero_for_one {
            MIN_SQRT_RATIO + U256::from(1)
        } else {
            MAX_SQRT_RATIO - U256::from(1)
        };

        let (amount0_delta, amount1_delta, _final_state) = self
            ._calculate_swap(zero_for_one, amount_specified, sqrt_price_limit_x96, None)
            .await?;

        Ok(if zero_for_one {
            amount0_delta.into_raw()
        } else {
            amount1_delta.into_raw()
        })
    }

    async fn nominal_price(&self) -> Result<f64, ArbRsError> {
        let absolute_price = self.absolute_price().await?;
        let scaling_factor =
            10_f64.powi(self.token0.decimals() as i32 - self.token1.decimals() as i32);
        Ok(absolute_price * scaling_factor)
    }

    async fn absolute_price(&self) -> Result<f64, ArbRsError> {
        let state = self.state.read().await;
        if state.sqrt_price_x96.is_zero() {
            return Ok(0.0);
        }

        let sqrt_price_x96_f64: f64 = state.sqrt_price_x96.to_string().parse().map_err(|_| {
            ArbRsError::CalculationError("Failed to parse sqrt_price_x96 to f64".to_string())
        })?;

        let q96: U256 = U256::from(1) << 96;
        let q96_f64: f64 = q96
            .to_string()
            .parse()
            .map_err(|_| ArbRsError::CalculationError("Failed to parse Q96 to f64".to_string()))?;

        if q96_f64 == 0.0 {
            return Err(ArbRsError::CalculationError(
                "Q96 is zero, division impossible".to_string(),
            ));
        }

        let ratio = sqrt_price_x96_f64 / q96_f64;
        Ok(ratio.powi(2))
    }

    async fn absolute_exchange_rate(&self) -> Result<f64, ArbRsError> {
        let price = self.absolute_price().await?;
        if price == 0.0 {
            Ok(f64::INFINITY)
        } else {
            Ok(1.0 / price)
        }
    }
}

impl<P: Provider + Send + Sync + 'static + ?Sized> Debug for UniswapV3Pool<P> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.debug_struct("UniswapV3Pool")
            .field("address", &self.address)
            .field("token0", &self.token0.symbol())
            .field("token1", &self.token1.symbol())
            .field("fee", &self.fee)
            .field("tick_spacing", &self.tick_spacing)
            .finish_non_exhaustive()
    }
}
