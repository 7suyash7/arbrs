use crate::core::token::{Token, TokenLike};
use crate::errors::ArbRsError;
use crate::pool::LiquidityPool;
use alloy_primitives::{keccak256, Address, Bytes, TxKind, B256, U256};
use alloy_provider::Provider;
use alloy_rpc_types::{BlockId, TransactionRequest};
use alloy_sol_types::{sol, SolCall};
use async_trait::async_trait;
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

/// Uniswap V2 Pool Struct Definition
pub struct UniswapV2Pool<P: ?Sized> {
    address: Address,
    token0: Arc<Token<P>>,
    token1: Arc<Token<P>>,
    state: RwLock<UniswapV2PoolState>,
    provider: Arc<P>,
    fee: u32,
}

impl<P: Provider + Send + Sync + ?Sized + 'static> UniswapV2Pool<P> {
    /// Creates a new instance of the Uniswap V2 pool.
    ///
    /// # Arguments
    /// * `address`: The pool contract address.
    /// * `token0`: Arc to the first token in the pair.
    /// * `token1`: Arc to the second token in the pair.
    /// * `provider`: Shared provider instance.
    /// * `fee`: Optional custom fee per mille (1/1000). Defaults to 3 (0.3%).
    pub fn new(
        address: Address,
        token0: Arc<Token<P>>,
        token1: Arc<Token<P>>,
        provider: Arc<P>,
        fee: Option<u32>,
    ) -> Self {
        const V2_DEFAULT_FEE_PER_MILLE: u32 = 3;
        Self {
            address,
            token0,
            token1,
            state: RwLock::new(UniswapV2PoolState::default()),
            provider,
            fee: fee.unwrap_or(V2_DEFAULT_FEE_PER_MILLE),
        }
    }

    /// Calculates the deterministic pool address for a given pair of tokens and factory parameters.
    pub fn calculate_pool_address(
        token_a: Address,
        token_b: Address,
        factory_address: Address,
        init_code_hash: B256,
    ) -> Address {
        let (token0, token1) = if token_a < token_b { (token_a, token_b) } else { (token_b, token_a) };
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

    /// Calculates swap output using a provided state object, bypassing the internal cached state.
    pub fn calculate_tokens_out_with_override(
        &self,
        token_in: &Token<P>,
        amount_in: U256,
        override_state: &UniswapV2PoolState,
    ) -> Result<U256, ArbRsError> {
        self.validate_token_in(token_in)?;
        self.calculate_swap_output_pure(token_in.address(), amount_in, override_state)
    }

    /// Calculates swap input using a provided state object, bypassing the internal cached state.
    pub fn calculate_tokens_in_from_tokens_out_with_override(
        &self,
        token_out: &Token<P>,
        amount_out: U256,
        override_state: &UniswapV2PoolState,
    ) -> Result<U256, ArbRsError> {
        self.validate_token_out(token_out)?;
        self.calculate_swap_input_pure(token_out.address(), amount_out, override_state)
    }
    
    /// Returns a clone of the current cached reserves (reserve0, reserve1).
    pub async fn get_cached_reserves(&self) -> UniswapV2PoolState {
        self.state.read().await.clone()
    }

    /// Pure calculation function for exact input swaps. Assumes validation already happened.
    fn calculate_swap_output_pure(
        &self,
        token_in_address: Address,
        amount_in: U256,
        reserves: &UniswapV2PoolState,
    ) -> Result<U256, ArbRsError> {
        let (reserve_in, reserve_out) = if token_in_address == self.token0.address() {
            (reserves.reserve0, reserves.reserve1)
        } else {
            (reserves.reserve1, reserves.reserve0)
        };

        if amount_in == U256::ZERO { return Err(ArbRsError::CalculationError("Input amount cannot be zero".to_string())); }
        if reserve_in == U256::ZERO || reserve_out == U256::ZERO { return Err(ArbRsError::CalculationError("Pool reserves cannot be zero".to_string())); }

        let fee_denominator = U256::from(1000);
        let amount_in_with_fee = amount_in.checked_mul(fee_denominator.saturating_sub(U256::from(self.fee)))
            .ok_or_else(|| ArbRsError::CalculationError("Overflow calculating amount with fee".to_string()))?;
        let numerator = amount_in_with_fee.checked_mul(reserve_out)
            .ok_or_else(|| ArbRsError::CalculationError("Overflow calculating numerator".to_string()))?;
        let denominator = reserve_in.checked_mul(fee_denominator)
            .ok_or_else(|| ArbRsError::CalculationError("Overflow calculating denominator part 1".to_string()))?
            .checked_add(amount_in_with_fee)
            .ok_or_else(|| ArbRsError::CalculationError("Overflow calculating denominator part 2".to_string()))?;

        if denominator == U256::ZERO { return Err(ArbRsError::CalculationError("Division by zero in calculation".to_string())); }
        Ok(numerator / denominator)
    }

    /// Pure calculation function for exact output swaps. Assumes validation already happened.
    fn calculate_swap_input_pure(
        &self,
        token_out_address: Address,
        amount_out: U256,
        reserves: &UniswapV2PoolState,
    ) -> Result<U256, ArbRsError> {
        let (reserve_in, reserve_out) = if token_out_address == self.token1.address() {
            (reserves.reserve0, reserves.reserve1)
        } else {
            (reserves.reserve1, reserves.reserve0)
        };

        if amount_out == U256::ZERO { return Err(ArbRsError::CalculationError("Output amount cannot be zero".to_string())); }
        if reserve_in == U256::ZERO || reserve_out == U256::ZERO { return Err(ArbRsError::CalculationError("Pool reserves cannot be zero".to_string())); }
        if amount_out >= reserve_out { return Err(ArbRsError::CalculationError("Insufficient liquidity for desired output amount".to_string())); }

        let fee_denominator = U256::from(1000);
        let fee_numerator = U256::from(self.fee);
        let numerator = reserve_in.checked_mul(amount_out)
            .ok_or_else(|| ArbRsError::CalculationError("Overflow calculating numerator part 1".to_string()))?
            .checked_mul(fee_denominator)
            .ok_or_else(|| ArbRsError::CalculationError("Overflow calculating numerator part 2".to_string()))?;
        let denominator = reserve_out.checked_sub(amount_out)
            .ok_or_else(|| ArbRsError::CalculationError("Underflow calculating denominator part 1".to_string()))?
            .checked_mul(fee_denominator.saturating_sub(fee_numerator))
            .ok_or_else(|| ArbRsError::CalculationError("Overflow calculating denominator part 2".to_string()))?;
        if denominator == U256::ZERO { return Err(ArbRsError::CalculationError("Division by zero in calculation".to_string())); }
        
        let amount_in = numerator.checked_div(denominator)
            .ok_or_else(|| ArbRsError::CalculationError("Division error during final calculation".to_string()))?
            .checked_add(U256::from(1))
            .ok_or_else(|| ArbRsError::CalculationError("Overflow adding safety margin".to_string()))?;
        Ok(amount_in)
    }
    
    fn validate_token_in(&self, token_in: &Token<P>) -> Result<(), ArbRsError> {
        if token_in.address() != self.token0.address() && token_in.address() != self.token1.address() {
            Err(ArbRsError::CalculationError(format!("Input token {} is not part of this pool", token_in.address())))
        } else {
            Ok(())
        }
    }

    fn validate_token_out(&self, token_out: &Token<P>) -> Result<(), ArbRsError> {
        if token_out.address() != self.token0.address() && token_out.address() != self.token1.address() {
            Err(ArbRsError::CalculationError(format!("Output token {} is not part of this pool", token_out.address())))
        } else {
            Ok(())
        }
    }
}

#[async_trait]
impl<P: Provider + Send + Sync + 'static + ?Sized> LiquidityPool<P> for UniswapV2Pool<P> {
    fn address(&self) -> Address {
        self.address
    }

    fn tokens(&self) -> (Arc<Token<P>>, Arc<Token<P>>) {
        (self.token0.clone(), self.token1.clone())
    }

    async fn update_state(&self) -> Result<(), ArbRsError> {
        let call = getReservesCall {};
        let request = TransactionRequest {
            to: Some(TxKind::Call(self.address)),
            input: Some(Bytes::from(call.abi_encode())).into(),
            ..Default::default()
        };
        let result_bytes = self.provider.call(request).block(BlockId::latest()).await
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
        self.calculate_swap_output_pure(token_in.address(), amount_in, &current_state)
    }

    async fn calculate_tokens_in_from_tokens_out(
        &self,
        token_out: &Token<P>,
        amount_out: U256,
    ) -> Result<U256, ArbRsError> {
        self.validate_token_out(token_out)?;
        let current_state = self.state.read().await;
        self.calculate_swap_input_pure(token_out.address(), amount_out, &current_state)
    }

    async fn absolute_price(&self) -> Result<f64, ArbRsError> {
        let current_state = self.state.read().await;
        if current_state.reserve0 == U256::ZERO { return Err(ArbRsError::CalculationError("Cannot calculate price: reserve0 is zero".to_string())); }
        let reserve0_f64 = current_state.reserve0.to_string().parse::<f64>().unwrap_or(0.0);
        let reserve1_f64 = current_state.reserve1.to_string().parse::<f64>().unwrap_or(0.0);
        if reserve0_f64 == 0.0 { return Err(ArbRsError::CalculationError("Cannot calculate price: reserve0 conversion failed or is zero".to_string())); }
        Ok(reserve1_f64 / reserve0_f64)
    }

    async fn nominal_price(&self) -> Result<f64, ArbRsError> {
        let absolute_price = self.absolute_price().await?;
        let scaling_factor = 10_f64.powi(self.token0.decimals() as i32 - self.token1.decimals() as i32);
        Ok(absolute_price * scaling_factor)
    }
}

impl<P: ?Sized> Debug for UniswapV2Pool<P> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.debug_struct("UniswapV2Pool")
            .field("address", &self.address)
            .field("fee", &self.fee)
            .finish_non_exhaustive()
    }
}