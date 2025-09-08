use crate::core::token::{Token, TokenLike};
use crate::errors::ArbRsError;
use crate::pool::LiquidityPool;
use crate::pool::strategy::V2CalculationStrategy;
use alloy_primitives::{Address, B256, Bytes, TxKind, U256, keccak256};
use alloy_provider::Provider;
use alloy_rpc_types::{BlockId, TransactionRequest};
use alloy_sol_types::{SolCall, sol};
use async_trait::async_trait;
use std::any::Any;
use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::sync::Arc;
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
}

pub struct UniswapV2Pool<P: ?Sized, S: V2CalculationStrategy> {
    address: Address,
    token0: Arc<Token<P>>,
    token1: Arc<Token<P>>,
    state: RwLock<UniswapV2PoolState>,
    provider: Arc<P>,
    strategy: S,
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
        }
    }

    /// Calculates swap output using a provided state object, bypassing the internal cached state.
    pub fn calculate_tokens_out_with_override(
        &self,
        token_in: &Token<P>,
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
}

#[async_trait]
impl<P: Provider + Send + Sync + ?Sized + 'static, S: V2CalculationStrategy + 'static>
    LiquidityPool<P> for UniswapV2Pool<P, S>
{
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

        let mut state = self.state.write().await;
        state.reserve0 = U256::from(decoded.reserve0);
        state.reserve1 = U256::from(decoded.reserve1);
        Ok(())
    }

    async fn calculate_tokens_out(
        &self,
        token_in: &Token<P>,
        amount_in: U256,
    ) -> Result<U256, ArbRsError> {
        self.validate_token_in(token_in)?;
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
        token_out: &Token<P>,
        amount_out: U256,
    ) -> Result<U256, ArbRsError> {
        self.validate_token_out(token_out)?;
        let current_state = self.state.read().await;
        let (reserve_in, reserve_out) = if token_out.address() == self.token1.address() {
            (current_state.reserve0, current_state.reserve1)
        } else {
            (current_state.reserve1, current_state.reserve0)
        };
        self.strategy
            .calculate_tokens_in_from_tokens_out(reserve_in, reserve_out, amount_out)
    }

    async fn absolute_price(&self) -> Result<f64, ArbRsError> {
        let current_state = self.state.read().await;
        if current_state.reserve0 == U256::ZERO {
            return Err(ArbRsError::CalculationError(
                "Cannot calculate price: reserve0 is zero".to_string(),
            ));
        }
        let reserve0_f64 = current_state
            .reserve0
            .to_string()
            .parse::<f64>()
            .unwrap_or(0.0);
        let reserve1_f64 = current_state
            .reserve1
            .to_string()
            .parse::<f64>()
            .unwrap_or(0.0);
        if reserve0_f64 == 0.0 {
            return Err(ArbRsError::CalculationError(
                "Cannot calculate price: reserve0 conversion failed or is zero".to_string(),
            ));
        }
        Ok(reserve1_f64 / reserve0_f64)
    }

    async fn nominal_price(&self) -> Result<f64, ArbRsError> {
        let absolute_price = self.absolute_price().await?;
        let scaling_factor =
            10_f64.powi(self.token0.decimals() as i32 - self.token1.decimals() as i32);
        Ok(absolute_price * scaling_factor)
    }
}

impl<P: ?Sized, S: V2CalculationStrategy> Debug for UniswapV2Pool<P, S> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.debug_struct("UniswapV2Pool")
            .field("address", &self.address)
            .field("strategy", &self.strategy) // Updated debug output
            .finish_non_exhaustive()
    }
}
