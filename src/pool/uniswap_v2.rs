use crate::core::messaging::{Publisher, PublisherMessage, Subscriber};
use crate::core::token::{Token, TokenLike};
use crate::errors::ArbRsError;
use crate::math::v3::full_math;
use crate::pool::LiquidityPool;
use crate::pool::strategy::V2CalculationStrategy;
use crate::pool::uniswap_v2_simulation::UniswapV2PoolSimulationResult;
use alloy_primitives::{Address, B256, Bytes, I256, TxKind, U256, keccak256};
use alloy_provider::Provider;
use alloy_rpc_types::{BlockId, BlockNumberOrTag, TransactionRequest};
use alloy_sol_types::{SolCall, sol};
use async_trait::async_trait;
use std::any::Any;
use std::collections::BTreeMap;
use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::sync::{Arc, Weak};
use tokio::sync::RwLock;

// ABI Definition
sol!(
    function getReserves() external view returns (uint112 reserve0, uint112 reserve1, uint32 blockTimestampLast);
);

/// Holds the reserves for a Uniswap V2 pool at a specific block.
#[derive(Clone, Debug, Default)]
pub struct UniswapV2PoolState {
    pub reserve0: U256,
    pub reserve1: U256,
    pub block_number: u64,
}

pub struct UniswapV2Pool<P: ?Sized, S: V2CalculationStrategy> {
    address: Address,
    token0: Arc<Token<P>>,
    token1: Arc<Token<P>>,
    state: RwLock<UniswapV2PoolState>,
    provider: Arc<P>,
    strategy: S,
    state_cache: RwLock<BTreeMap<u64, UniswapV2PoolState>>,
    subscribers: RwLock<Vec<Weak<dyn Subscriber<P>>>>,
}

#[async_trait]
impl<P: Provider + Send + Sync + 'static + ?Sized, S: V2CalculationStrategy + 'static> Publisher<P>
    for UniswapV2Pool<P, S>
{
    async fn subscribe(&self, subscriber: Weak<dyn Subscriber<P>>) {
        let mut subscribers = self.subscribers.write().await;
        subscribers.push(subscriber);
    }

    async fn unsubscribe(&self, subscriber_id: usize) {
        let mut subscribers = self.subscribers.write().await;
        subscribers.retain(|weak_sub| {
            if let Some(sub) = weak_sub.upgrade() {
                sub.id() != subscriber_id
            } else {
                false // remove dead weak pointers
            }
        });
    }

    async fn notify_subscribers(&self, message: PublisherMessage) {
        let subscribers = self.subscribers.read().await;
        for weak_sub in subscribers.iter() {
            if let Some(sub) = weak_sub.upgrade() {
                sub.notify(message.clone()).await;
            }
        }
    }
}

impl<P: Provider + Send + Sync + ?Sized + 'static, S: V2CalculationStrategy> UniswapV2Pool<P, S> {
    /// Creates a new instance of the Uniswap V2 pool.
    pub fn new(
        address: Address,
        token0: Arc<Token<P>>,
        token1: Arc<Token<P>>,
        provider: Arc<P>,
        strategy: S,
    ) -> Self {
        Self {
            address,
            token0,
            token1,
            state: RwLock::new(UniswapV2PoolState::default()),
            provider,
            strategy,
            state_cache: RwLock::new(BTreeMap::new()),
            subscribers: RwLock::new(Vec::new()),
        }
    }

    /// Calculates swap output using a provided state object, bypassing the internal cached state.
    pub fn calculate_tokens_out_with_override(
        &self,
        token_in: &Token<P>,
        _token_out: &Token<P>,
        amount_in: U256,
        override_state: &UniswapV2PoolState,
    ) -> Result<U256, ArbRsError> {
        self.validate_token_in(token_in)?;
        let (reserve_in, reserve_out) = if token_in.address() == self.token0.address() {
            (override_state.reserve0, override_state.reserve1)
        } else {
            (override_state.reserve1, override_state.reserve0)
        };
        self.strategy
            .calculate_tokens_out(reserve_in, reserve_out, amount_in)
    }

    /// Calculates swap input using a provided state object, bypassing the internal cached state.
    pub fn calculate_tokens_in_from_tokens_out_with_override(
        &self,
        _token_in: &Token<P>,
        token_out: &Token<P>,
        amount_out: U256,
        override_state: &UniswapV2PoolState,
    ) -> Result<U256, ArbRsError> {
        self.validate_token_out(token_out)?;
        let (reserve_in, reserve_out) = if token_out.address() == self.token1.address() {
            (override_state.reserve0, override_state.reserve1)
        } else {
            (override_state.reserve1, override_state.reserve0)
        };
        self.strategy
            .calculate_tokens_in_from_tokens_out(reserve_in, reserve_out, amount_out)
    }

    /// Returns a clone of the current cached reserves (reserve0, reserve1).
    pub async fn get_cached_reserves(&self) -> UniswapV2PoolState {
        self.state.read().await.clone()
    }

    fn validate_token_in(&self, token_in: &Token<P>) -> Result<(), ArbRsError> {
        if token_in.address() != self.token0.address()
            && token_in.address() != self.token1.address()
        {
            Err(ArbRsError::CalculationError(format!(
                "Input token {} is not part of this pool",
                token_in.address()
            )))
        } else {
            Ok(())
        }
    }

    fn validate_token_out(&self, token_out: &Token<P>) -> Result<(), ArbRsError> {
        if token_out.address() != self.token0.address()
            && token_out.address() != self.token1.address()
        {
            Err(ArbRsError::CalculationError(format!(
                "Output token {} is not part of this pool",
                token_out.address()
            )))
        } else {
            Ok(())
        }
    }

    fn validate_token_pair(
        &self,
        token_a: &Token<P>,
        token_b: &Token<P>,
    ) -> Result<(), ArbRsError> {
        if !((token_a.address() == self.token0.address()
            && token_b.address() == self.token1.address())
            || (token_a.address() == self.token1.address()
                && token_b.address() == self.token0.address()))
        {
            Err(ArbRsError::CalculationError(
                "Token pair does not match pool".into(),
            ))
        } else {
            Ok(())
        }
    }

    /// Returns a reference to the pool's calculation strategy.
    pub fn strategy(&self) -> &S {
        &self.strategy
    }

    pub fn calculate_pool_address(
        token_a: Address,
        token_b: Address,
        factory_address: Address,
        init_code_hash: B256,
    ) -> Address {
        let (token0, token1) = if token_a < token_b {
            (token_a, token_b)
        } else {
            (token_b, token_a)
        };
        let mut packed = [0u8; 40];
        packed[..20].copy_from_slice(token0.as_slice());
        packed[20..].copy_from_slice(token1.as_slice());
        let salt = keccak256(packed);
        let mut data = Vec::with_capacity(85);
        data.push(0xff);
        data.extend_from_slice(factory_address.as_slice());
        data.extend_from_slice(salt.as_slice());
        data.extend_from_slice(init_code_hash.as_slice());
        Address::from_slice(&keccak256(data)[12..])
    }

    /// Restore the last pool state recorded prior to a target block.
    pub async fn restore_state_before_block(&self, block: u64) -> Result<(), ArbRsError> {
        let mut state_cache = self.state_cache.write().await;
        let mut keys_to_remove = Vec::new();

        for &block_number in state_cache.keys() {
            if block_number >= block {
                keys_to_remove.push(block_number);
            }
        }

        for key in keys_to_remove {
            state_cache.remove(&key);
        }

        if let Some((&latest_block, latest_state)) = state_cache.iter().next_back() {
            let mut current_state = self.state.write().await;
            *current_state = latest_state.clone();
            current_state.block_number = latest_block;
            Ok(())
        } else {
            Err(ArbRsError::NoPoolStateAvailable(block))
        }
    }

    /// Discard states recorded prior to a target block.
    pub async fn discard_states_before_block(&self, block: u64) {
        let mut state_cache = self.state_cache.write().await;
        let keys_to_remove: Vec<u64> = state_cache
            .keys()
            .filter(|&&b| b < block)
            .cloned()
            .collect();
        for key in keys_to_remove {
            state_cache.remove(&key);
        }
    }

    pub async fn calculate_tokens_in_from_ratio_out(
        &self,
        token_in: &Token<P>,
        ratio_absolute: f64,
    ) -> Result<U256, ArbRsError> {
        self.validate_token_in(token_in)?;
        let current_state = self.state.read().await;

        let (reserve_in, reserve_out) = if token_in.address() == self.token0.address() {
            (current_state.reserve0, current_state.reserve1)
        } else {
            (current_state.reserve1, current_state.reserve0)
        };

        if ratio_absolute <= 0.0 {
            return Err(ArbRsError::CalculationError(
                "Ratio must be positive".to_string(),
            ));
        }

        let reserve_in_f = reserve_in.to_string().parse::<f64>().unwrap_or(0.0);
        let reserve_out_f = reserve_out.to_string().parse::<f64>().unwrap_or(0.0);

        let fee = self.strategy.get_fee_bps() as f64 / 10000.0;

        let amount_in = reserve_out_f / ratio_absolute - reserve_in_f / (1.0 - fee);

        if amount_in > 0.0 {
            Ok(U256::from(amount_in.floor() as u128))
        } else {
            Ok(U256::ZERO)
        }
    }

    pub async fn simulate_add_liquidity(
        &self,
        added_reserves_token0: U256,
        added_reserves_token1: U256,
        override_state: Option<&UniswapV2PoolState>,
    ) -> UniswapV2PoolSimulationResult {
        let state_guard = self.state.read().await;
        let initial_state = override_state.unwrap_or(&state_guard);

        println!(
            "[ADD LIQ SIM] Initial Reserves: r0={}, r1={}",
            initial_state.reserve0, initial_state.reserve1
        );

        let (amount0_actual, amount1_actual) =
            if initial_state.reserve0 == U256::ZERO && initial_state.reserve1 == U256::ZERO {
                (added_reserves_token0, added_reserves_token1)
            } else {
                let amount1_optimal = full_math::mul_div(
                    added_reserves_token0,
                    initial_state.reserve1,
                    initial_state.reserve0,
                )
                .unwrap_or(U256::MAX);

                if amount1_optimal <= added_reserves_token1 {
                    (added_reserves_token0, amount1_optimal)
                } else {
                    let amount0_optimal = full_math::mul_div(
                        added_reserves_token1,
                        initial_state.reserve0,
                        initial_state.reserve1,
                    )
                    .unwrap_or(U256::MAX);
                    (amount0_optimal, added_reserves_token1)
                }
            };

        let final_state = UniswapV2PoolState {
            reserve0: initial_state.reserve0 + amount0_actual,
            reserve1: initial_state.reserve1 + amount1_actual,
            block_number: initial_state.block_number,
        };

        UniswapV2PoolSimulationResult {
            amount0_delta: I256::from_raw(amount0_actual),
            amount1_delta: I256::from_raw(amount1_actual),
            initial_state: initial_state.clone(),
            final_state,
        }
    }

    pub async fn simulate_remove_liquidity(
        &self,
        removed_reserves_token0: U256,
        removed_reserves_token1: U256,
        override_state: Option<&UniswapV2PoolState>,
    ) -> UniswapV2PoolSimulationResult {
        let state_guard = self.state.read().await;
        let initial_state = override_state.unwrap_or(&state_guard);

        let final_state = UniswapV2PoolState {
            reserve0: initial_state
                .reserve0
                .saturating_sub(removed_reserves_token0),
            reserve1: initial_state
                .reserve1
                .saturating_sub(removed_reserves_token1),
            block_number: initial_state.block_number,
        };

        UniswapV2PoolSimulationResult {
            amount0_delta: -I256::from_raw(removed_reserves_token0),
            amount1_delta: -I256::from_raw(removed_reserves_token1),
            initial_state: initial_state.clone(),
            final_state,
        }
    }

    pub async fn simulate_exact_input_swap(
        &self,
        token_in: &Token<P>,
        token_out: &Token<P>,
        token_in_quantity: U256,
        override_state: Option<&UniswapV2PoolState>,
    ) -> Result<UniswapV2PoolSimulationResult, ArbRsError> {
        self.validate_token_pair(token_in, token_out)?;
        let state_guard = self.state.read().await;
        let initial_state = override_state.unwrap_or(&state_guard);

        println!(
            "[SWAP SIM] Initial Reserves: r0={}, r1={}",
            initial_state.reserve0, initial_state.reserve1
        );

        let token_out_quantity = self.calculate_tokens_out_with_override(
            token_in,
            token_out,
            token_in_quantity,
            initial_state,
        )?;

        let (final_reserve0, final_reserve1, amount0_delta, amount1_delta) =
            if token_in.address() == self.token0.address() {
                (
                    initial_state.reserve0 + token_in_quantity,
                    initial_state
                        .reserve1
                        .checked_sub(token_out_quantity)
                        .ok_or(ArbRsError::CalculationError(
                            "Swap would drain reserve1".to_string(),
                        ))?,
                    I256::from_raw(token_in_quantity),
                    -I256::from_raw(token_out_quantity),
                )
            } else {
                (
                    initial_state
                        .reserve0
                        .checked_sub(token_out_quantity)
                        .ok_or(ArbRsError::CalculationError(
                            "Swap would drain reserve0".to_string(),
                        ))?,
                    initial_state.reserve1 + token_in_quantity,
                    -I256::from_raw(token_out_quantity),
                    I256::from_raw(token_in_quantity),
                )
            };

        let final_state = UniswapV2PoolState {
            reserve0: final_reserve0,
            reserve1: final_reserve1,
            block_number: initial_state.block_number,
        };

        Ok(UniswapV2PoolSimulationResult {
            amount0_delta,
            amount1_delta,
            initial_state: initial_state.clone(),
            final_state,
        })
    }

    pub async fn simulate_exact_output_swap(
        &self,
        token_in: &Token<P>,
        token_out: &Token<P>,
        token_out_quantity: U256,
        override_state: Option<&UniswapV2PoolState>,
    ) -> Result<UniswapV2PoolSimulationResult, ArbRsError> {
        self.validate_token_pair(token_in, token_out)?;
        let state_guard = self.state.read().await;
        let initial_state = override_state.unwrap_or(&state_guard);

        let token_in_quantity = self.calculate_tokens_in_from_tokens_out_with_override(
            token_in,
            token_out,
            token_out_quantity,
            initial_state,
        )?;

        let (final_reserve0, final_reserve1, amount0_delta, amount1_delta) =
            if token_out.address() == self.token1.address() {
                (
                    initial_state.reserve0 + token_in_quantity,
                    initial_state
                        .reserve1
                        .checked_sub(token_out_quantity)
                        .ok_or(ArbRsError::CalculationError(
                            "Swap would drain reserve1".to_string(),
                        ))?,
                    I256::from_raw(token_in_quantity),
                    -I256::from_raw(token_out_quantity),
                )
            } else {
                (
                    initial_state
                        .reserve0
                        .checked_sub(token_out_quantity)
                        .ok_or(ArbRsError::CalculationError(
                            "Swap would drain reserve0".to_string(),
                        ))?,
                    initial_state.reserve1 + token_in_quantity,
                    -I256::from_raw(token_out_quantity),
                    I256::from_raw(token_in_quantity),
                )
            };

        let final_state = UniswapV2PoolState {
            reserve0: final_reserve0,
            reserve1: final_reserve1,
            block_number: initial_state.block_number,
        };

        Ok(UniswapV2PoolSimulationResult {
            amount0_delta,
            amount1_delta,
            initial_state: initial_state.clone(),
            final_state,
        })
    }

    /// Fetches reserves at a specific block number without updating the live state.
    pub async fn _fetch_state_at_block(
        &self,
        block_number: u64,
    ) -> Result<UniswapV2PoolState, ArbRsError> {
        let call = getReservesCall {};
        let request = TransactionRequest {
            to: Some(TxKind::Call(self.address)),
            input: Some(Bytes::from(call.abi_encode())).into(),
            ..Default::default()
        };
        let result_bytes = self
            .provider
            .call(request)
            .block(BlockId::Number(BlockNumberOrTag::Number(block_number)))
            .await
            .map_err(|e| ArbRsError::ProviderError(e.to_string()))?;
        let decoded = getReservesCall::abi_decode_returns(&result_bytes)
            .map_err(|e| ArbRsError::AbiDecodeError(e.to_string()))?;
        Ok(UniswapV2PoolState {
            reserve0: U256::from(decoded.reserve0),
            reserve1: U256::from(decoded.reserve1),
            block_number,
        })
    }

    /// Fetches state at a specific block and adds it to the cache.
    /// Used for populating historical data for simulations.
    pub async fn fetch_and_cache_state_at_block(
        &self,
        block_number: u64,
    ) -> Result<UniswapV2PoolState, ArbRsError> {
        let new_state = self._fetch_state_at_block(block_number).await?;
        let mut cache = self.state_cache.write().await;
        cache.insert(block_number, new_state.clone());
        Ok(new_state)
    }
}

#[async_trait]
impl<P: Provider + Send + Sync + ?Sized + 'static, S: V2CalculationStrategy + 'static>
    LiquidityPool<P> for UniswapV2Pool<P, S>
{
    fn address(&self) -> Address {
        self.address
    }

    fn get_all_tokens(&self) -> Vec<Arc<Token<P>>> {
        vec![self.token0.clone(), self.token1.clone()]
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

        let call = getReservesCall {};
        let request = TransactionRequest {
            to: Some(TxKind::Call(self.address)),
            input: Some(Bytes::from(call.abi_encode())).into(),
            ..Default::default()
        };

        let result_bytes = self
            .provider
            .call(request)
            .block(BlockId::latest())
            .await
            .map_err(|e| ArbRsError::ProviderError(e.to_string()))?;

        let decoded = getReservesCall::abi_decode_returns(&result_bytes)
            .map_err(|e| ArbRsError::AbiDecodeError(e.to_string()))?;

        let new_state = UniswapV2PoolState {
            reserve0: U256::from(decoded.reserve0),
            reserve1: U256::from(decoded.reserve1),
            block_number: latest_block,
        };

        let (state_updated, _old_state) = {
            let state = self.state.read().await;
            (
                state.reserve0 != new_state.reserve0 || state.reserve1 != new_state.reserve1,
                state.clone(),
            )
        };

        if state_updated {
            let mut state_writer = self.state.write().await;
            *state_writer = new_state.clone();

            let mut cache = self.state_cache.write().await;
            cache.insert(latest_block, new_state.clone());

            self.notify_subscribers(PublisherMessage::PoolStateUpdate(new_state))
                .await;
        }

        Ok(())
    }

    async fn calculate_tokens_out(
        &self,
        token_in: &Token<P>,
        token_out: &Token<P>,
        amount_in: U256,
    ) -> Result<U256, ArbRsError> {
        self.validate_token_pair(token_in, token_out)?;
        let current_state = self.state.read().await;
        let (reserve_in, reserve_out) = if token_in.address() == self.token0.address() {
            (current_state.reserve0, current_state.reserve1)
        } else {
            (current_state.reserve1, current_state.reserve0)
        };
        self.strategy
            .calculate_tokens_out(reserve_in, reserve_out, amount_in)
    }

    async fn calculate_tokens_in_from_tokens_out(
        &self,
        token_in: &Token<P>,
        token_out: &Token<P>,
        amount_out: U256,
    ) -> Result<U256, ArbRsError> {
        self.validate_token_pair(token_in, token_out)?;
        let current_state = self.state.read().await;
        let (reserve_in, reserve_out) = if token_out.address() == self.token1.address() {
            (current_state.reserve0, current_state.reserve1)
        } else {
            (current_state.reserve1, current_state.reserve0)
        };
        self.strategy
            .calculate_tokens_in_from_tokens_out(reserve_in, reserve_out, amount_out)
    }

    async fn absolute_price(
        &self,
        token_in: &Token<P>,
        token_out: &Token<P>,
    ) -> Result<f64, ArbRsError> {
        self.validate_token_pair(token_in, token_out)?;
        let current_state = self.state.read().await;
        let (reserve_in, reserve_out) = if token_in.address() == self.token0.address() {
            (current_state.reserve0, current_state.reserve1)
        } else {
            (current_state.reserve1, current_state.reserve0)
        };

        if reserve_in == U256::ZERO {
            return Err(ArbRsError::CalculationError(
                "Cannot calculate price: input reserve is zero".into(),
            ));
        }
        let reserve_in_f64 = reserve_in.to_string().parse::<f64>().unwrap_or(0.0);
        let reserve_out_f64 = reserve_out.to_string().parse::<f64>().unwrap_or(0.0);
        if reserve_in_f64 == 0.0 {
            return Err(ArbRsError::CalculationError(
                "Cannot calculate price: reserve conversion failed or is zero".into(),
            ));
        }
        Ok(reserve_out_f64 / reserve_in_f64)
    }

    async fn nominal_price(
        &self,
        token_in: &Token<P>,
        token_out: &Token<P>,
    ) -> Result<f64, ArbRsError> {
        let absolute_price = self.absolute_price(token_in, token_out).await?;
        let scaling_factor = 10_f64.powi(token_in.decimals() as i32 - token_out.decimals() as i32);
        Ok(absolute_price * scaling_factor)
    }

    async fn absolute_exchange_rate(
        &self,
        token_in: &Token<P>,
        token_out: &Token<P>,
    ) -> Result<f64, ArbRsError> {
        let price = self.absolute_price(token_in, token_out).await?;
        if price == 0.0 {
            Ok(f64::INFINITY)
        } else {
            Ok(1.0 / price)
        }
    }
}

impl<P: ?Sized, S: V2CalculationStrategy> Debug for UniswapV2Pool<P, S> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.debug_struct("UniswapV2Pool")
            .field("address", &self.address)
            .field("strategy", &self.strategy)
            .finish_non_exhaustive()
    }
}

pub struct UnregisteredLiquidityPool<P: ?Sized> {
    address: Address,
    token0: Arc<Token<P>>,
    token1: Arc<Token<P>>,
}

impl<P: Provider + Send + Sync + ?Sized + 'static> UnregisteredLiquidityPool<P> {
    pub fn new(address: Address, token0: Arc<Token<P>>, token1: Arc<Token<P>>) -> Self {
        Self {
            address,
            token0,
            token1,
        }
    }
}

#[async_trait]
impl<P: Provider + Send + Sync + ?Sized + 'static> LiquidityPool<P>
    for UnregisteredLiquidityPool<P>
{
    fn address(&self) -> Address {
        self.address
    }

    fn get_all_tokens(&self) -> Vec<Arc<Token<P>>> {
        vec![self.token0.clone(), self.token1.clone()]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    async fn update_state(&self) -> Result<(), ArbRsError> {
        Ok(())
    }

    async fn calculate_tokens_out(
        &self,
        _token_in: &Token<P>,
        _token_out: &Token<P>,
        _amount_in: U256,
    ) -> Result<U256, ArbRsError> {
        Err(ArbRsError::CalculationError(
            "Cannot calculate output for unregistered pool".into(),
        ))
    }

    async fn calculate_tokens_in_from_tokens_out(
        &self,
        _token_in: &Token<P>,
        _token_out: &Token<P>,
        _amount_out: U256,
    ) -> Result<U256, ArbRsError> {
        Err(ArbRsError::CalculationError(
            "Cannot calculate input for unregistered pool".into(),
        ))
    }

    async fn nominal_price(
        &self,
        _token_in: &Token<P>,
        _token_out: &Token<P>,
    ) -> Result<f64, ArbRsError> {
        Err(ArbRsError::CalculationError(
            "Cannot get price for unregistered pool".into(),
        ))
    }

    async fn absolute_price(
        &self,
        _token_in: &Token<P>,
        _token_out: &Token<P>,
    ) -> Result<f64, ArbRsError> {
        Err(ArbRsError::CalculationError(
            "Cannot get price for unregistered pool".into(),
        ))
    }

    async fn absolute_exchange_rate(
        &self,
        _token_in: &Token<P>,
        _token_out: &Token<P>,
    ) -> Result<f64, ArbRsError> {
        Err(ArbRsError::CalculationError(
            "Cannot get exchange rate for unregistered pool".into(),
        ))
    }
}

impl<P: ?Sized> Debug for UnregisteredLiquidityPool<P> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.debug_struct("UnregisteredLiquidityPool")
            .field("address", &self.address)
            .finish()
    }
}
