use crate::core::token::Token;
use async_recursion::async_recursion;
use crate::curve::attributes_builder;
use crate::curve::constants::{FEE_DENOMINATOR, PRECISION};
use crate::curve::math;
use crate::curve::pool_attributes::{PoolAttributes, SwapStrategyType};
use crate::curve::pool_overrides;
use crate::curve::registry::CurveRegistry;
use crate::curve::strategies::{
    AdminFeeStrategy, DefaultStrategy, DynamicFeeStrategy, LendingStrategy, MetapoolStrategy,
    OracleStrategy, SwapParams, SwapStrategy, TricryptoStrategy, UnscaledStrategy,
};
use crate::errors::ArbRsError;
use crate::manager::token_manager::TokenManager;
use crate::pool::LiquidityPool;
use crate::TokenLike;
use alloy_primitives::{address, Address, U256};
use alloy_provider::Provider;
use alloy_rpc_types::{BlockId, TransactionRequest};
use alloy_sol_types::{sol, SolCall};
use async_trait::async_trait;
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

const WETH_ADDRESS: Address = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
const NATIVE_PLACEHOLDERS: &[Address] = &[
Address::ZERO,
address!("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
];

sol! {
    function A() external view returns (uint256);
    function fee() external view returns (uint256);
    function coins(uint256 i) external view returns (address);
    function coins(int128 i) external view returns (address);
    function balances(uint256 i) external view returns (uint256);
    function balances(int128 i) external view returns (uint256);
    function get_virtual_price() external view returns (uint256);
    function exchangeRateStored() external view returns (uint256);
    function initial_A() external view returns (uint256);
    function initial_A_time() external view returns (uint256);
    function future_A() external view returns (uint256);
    function future_A_time() external view returns (uint256);
    function redemption_price_snap() external view returns (address);
    function snappedRedemptionPrice() external view returns (uint256);
    function admin_balances(uint256 i) external view returns (uint256);
    function admin_balances(int128 i) external view returns (uint256);
    function D() external view returns (uint256);
    function gamma() external view returns (uint256);
    function price_scale(uint256 i) external view returns (uint256);
    function oracle_method() external view returns (uint256);
    function price_oracle(uint256 i) external view returns (uint256);
    function supplyRatePerBlock() external view returns (uint256);
    function accrualBlockNumber() external view returns (uint256);
}

#[derive(Debug, Clone, Copy)]
pub struct ARampingState {
    pub initial_a: U256,
    pub initial_a_time: U256,
    pub future_a: U256,
    pub future_a_time: U256,
}

pub struct CurveStableswapPool<P: Provider + Send + Sync + 'static + ?Sized> {
    pub address: Address,
    pub lp_token: Arc<Token<P>>,
    pub tokens: Vec<Arc<Token<P>>>,
    pub underlying_tokens: Vec<Arc<Token<P>>>,
    pub provider: Arc<P>,
    pub token_manager: Arc<TokenManager<P>>,
    pub attributes: PoolAttributes,
    pub base_pool: Option<Arc<CurveStableswapPool<P>>>,
    a_ramping_state: Option<ARampingState>,
    pub a: RwLock<U256>,
    pub fee: RwLock<U256>,
    pub balances: RwLock<Vec<U256>>,
    pub cached_virtual_price: RwLock<Option<U256>>,
    cached_scaled_redemption_price: RwLock<HashMap<u64, U256>>,
    cached_tricrypto_d: RwLock<HashMap<u64, U256>>,
    cached_tricrypto_gamma: RwLock<HashMap<u64, U256>>,
    cached_tricrypto_price_scale: RwLock<HashMap<u64, Vec<U256>>>,
    pub cached_oracle_rates: RwLock<HashMap<u64, Vec<U256>>>,
}

#[async_trait]
impl<P: Provider + Send + Sync + 'static + ?Sized> LiquidityPool<P> for CurveStableswapPool<P> {
    fn address(&self) -> Address { self.address }
    fn get_all_tokens(&self) -> Vec<Arc<Token<P>>> { self.tokens.clone() }
    fn as_any(&self) -> &dyn Any { self }

    async fn update_state(&self) -> Result<(), ArbRsError> {
        let a_call = ACall {};
        let a_bytes = self.provider.call(TransactionRequest::default().to(self.address).input(a_call.abi_encode().into())).await?;
        *self.a.write().await = ACall::abi_decode_returns(&a_bytes)?;

        let fee_call = feeCall {};
        let fee_bytes = self.provider.call(TransactionRequest::default().to(self.address).input(fee_call.abi_encode().into())).await?;
        *self.fee.write().await = feeCall::abi_decode_returns(&fee_bytes)?;

        let live_balances = self.fetch_balances().await?;
        *self.balances.write().await = live_balances;

        if let Some(base_pool) = &self.base_pool {
            let vp_call = get_virtual_priceCall {};
            let request = TransactionRequest::default().to(base_pool.address).input(vp_call.abi_encode().into());
            let vp_bytes = self.provider.call(request).await?;
            *self.cached_virtual_price.write().await = Some(get_virtual_priceCall::abi_decode_returns(&vp_bytes)?);
        }
        Ok(())
    }

    async fn calculate_tokens_out(&self, token_in: &Token<P>, token_out: &Token<P>, amount_in: U256, block_number: Option<u64>) -> Result<U256, ArbRsError> {
        let is_metapool = self.base_pool.is_some();
        let is_direct_swap = self.tokens.iter().any(|t| t.address() == token_in.address()) && self.tokens.iter().any(|t| t.address() == token_out.address());

        if is_metapool && !is_direct_swap {
            return self.calculate_dy_underlying(token_in, token_out, amount_in, block_number).await;
        }

        let i = self.tokens.iter().position(|t| t.address() == token_in.address()).ok_or_else(|| ArbRsError::CalculationError("Token In not found in pool".to_string()))?;
        let j = self.tokens.iter().position(|t| t.address() == token_out.address()).ok_or_else(|| ArbRsError::CalculationError("Token Out not found in pool".to_string()))?;
        
        let block_num = if let Some(bn) = block_number { bn } else { self.provider.get_block_number().await? };
        
        let block_header = self.provider.get_block_by_number(block_num.into()).await?
            .ok_or_else(|| ArbRsError::DataFetchError(Address::ZERO))?;

        let params = SwapParams {
            i,
            j,
            dx: amount_in,
            pool: self,
            block_number: block_num,
            block_timestamp: block_header.header.timestamp,
        };

        match self.attributes.swap_strategy {
            SwapStrategyType::Default => DefaultStrategy::default().calculate_dy(&params).await,
            SwapStrategyType::Metapool => MetapoolStrategy::default().calculate_dy(&params).await,
            SwapStrategyType::Lending => LendingStrategy::default().calculate_dy(&params).await,
            SwapStrategyType::Unscaled => UnscaledStrategy::default().calculate_dy(&params).await,
            SwapStrategyType::DynamicFee => DynamicFeeStrategy::default().calculate_dy(&params).await,
            SwapStrategyType::Tricrypto => TricryptoStrategy::default().calculate_dy(&params).await,
            SwapStrategyType::Oracle => OracleStrategy::default().calculate_dy(&params).await,
            SwapStrategyType::AdminFee => AdminFeeStrategy::default().calculate_dy(&params).await,
        }
    }
    async fn calculate_tokens_in_from_tokens_out(&self, _token_in: &Token<P>, _token_out: &Token<P>, _amount_out: U256) -> Result<U256, ArbRsError> { unimplemented!() }
    async fn nominal_price(&self, _token_in: &Token<P>, _token_out: &Token<P>) -> Result<f64, ArbRsError> { unimplemented!() }
    async fn absolute_price(&self, _token_in: &Token<P>, _token_out: &Token<P>) -> Result<f64, ArbRsError> { unimplemented!() }
    async fn absolute_exchange_rate(&self, _token_in: &Token<P>, _token_out: &Token<P>) -> Result<f64, ArbRsError> { unimplemented!() }
}

impl<P: Provider + Send + Sync + 'static + ?Sized> CurveStableswapPool<P> {
    #[async_recursion]
    pub async fn new(address: Address, provider: Arc<P>, token_manager: Arc<TokenManager<P>>, registry: &CurveRegistry<P>) -> Result<Self, ArbRsError> {
        let tokens = Self::fetch_coins(&address, provider.clone(), &token_manager).await?;
        let lp_token = token_manager.get_token(registry.get_lp_token(address).await?).await?;
        let attributes = attributes_builder::build_attributes(address, &tokens, provider.clone(), &token_manager, registry).await?;

        let mut base_pool = None;
        if let Some(base_pool_address) = attributes.base_pool_address {
            let bp_instance = Self::new(base_pool_address, provider.clone(), token_manager.clone(), registry).await?;
            base_pool = Some(Arc::new(bp_instance));
        }

        let a_ramping_state = Self::fetch_a_ramping_state(address, provider.clone()).await?;

        let underlying_tokens = if let Some(bp) = &base_pool {
            let mut underlying = vec![tokens[0].clone()];
            underlying.extend(bp.tokens.clone());
            underlying
        } else {
            tokens.clone()
        };

        let pool = Self {
            address, lp_token, tokens, underlying_tokens, provider, token_manager, attributes, base_pool, a_ramping_state,
            a: RwLock::new(U256::ZERO), fee: RwLock::new(U256::ZERO), balances: RwLock::new(Vec::new()),
            cached_virtual_price: RwLock::new(None), 
            cached_scaled_redemption_price: RwLock::new(HashMap::new()),
            cached_tricrypto_d: RwLock::new(HashMap::new()),
            cached_tricrypto_gamma: RwLock::new(HashMap::new()),
            cached_tricrypto_price_scale: RwLock::new(HashMap::new()),
            cached_oracle_rates: RwLock::new(HashMap::new()),
        };
        pool.update_state().await?;
        Ok(pool)
    }

    pub async fn fetch_coins(address: &Address, provider: Arc<P>, token_manager: &TokenManager<P>) -> Result<Vec<Arc<Token<P>>>, ArbRsError> {
        let mut tokens = Vec::new();
        let mut use_int128 = true;
        let test_call_int = coins_1Call { i: 0 };
        if provider.call(TransactionRequest::default().to(*address).input(test_call_int.abi_encode().into())).await.is_err() {
            use_int128 = false;
        }

        for i in 0..8 {
            let result_bytes = if use_int128 {
                let call = coins_1Call { i: i as i128 };
                provider.call(TransactionRequest::default().to(*address).input(call.abi_encode().into())).await
            } else {
                let call = coins_0Call { i: U256::from(i) };
                provider.call(TransactionRequest::default().to(*address).input(call.abi_encode().into())).await
            };

            match result_bytes {
                Ok(bytes) => {
                    let mut token_address = if use_int128 { coins_1Call::abi_decode_returns(&bytes)? } else { coins_0Call::abi_decode_returns(&bytes)? };
                    if token_address.is_zero() { break; }
                    if NATIVE_PLACEHOLDERS.contains(&token_address) {
                        token_address = WETH_ADDRESS;
                    }
                    tokens.push(token_manager.get_token(token_address).await?);
                }
                Err(_) => break,
            }
        }
        if tokens.is_empty() { return Err(ArbRsError::DataFetchError(*address)); }
        Ok(tokens)
    }

    async fn fetch_a_ramping_state(
        address: Address,
        provider: Arc<P>,
    ) -> Result<Option<ARampingState>, ArbRsError> {
        let initial_a_call = initial_ACall {};
        let initial_a_bytes = match provider.call(TransactionRequest::default().to(address).input(initial_a_call.abi_encode().into())).await {
            Ok(bytes) => bytes,
            Err(_) => return Ok(None),
        };
        let initial_a = initial_ACall::abi_decode_returns(&initial_a_bytes)?;

        let initial_a_time_call = initial_A_timeCall {};
        let iat_bytes = provider.call(TransactionRequest::default().to(address).input(initial_a_time_call.abi_encode().into())).await?;
        let initial_a_time = initial_A_timeCall::abi_decode_returns(&iat_bytes)?;

        let future_a_call = future_ACall {};
        let fa_bytes = provider.call(TransactionRequest::default().to(address).input(future_a_call.abi_encode().into())).await?;
        let future_a = future_ACall::abi_decode_returns(&fa_bytes)?;

        let future_a_time_call = future_A_timeCall {};
        let fat_bytes = provider.call(TransactionRequest::default().to(address).input(future_a_time_call.abi_encode().into())).await?;
        let future_a_time = future_A_timeCall::abi_decode_returns(&fat_bytes)?;

        Ok(Some(ARampingState {
            initial_a,
            initial_a_time,
            future_a,
            future_a_time,
        }))
    }

    async fn update_state(&self) -> Result<(), ArbRsError> {
        let _block_number = self.provider.get_block_number().await?;

        let a_call = ACall {};
        let a_bytes = self.provider.call(TransactionRequest::default().to(self.address).input(a_call.abi_encode().into())).await?;
        *self.a.write().await = ACall::abi_decode_returns(&a_bytes)?;

        let fee_call = feeCall {};
        let fee_bytes = self.provider.call(TransactionRequest::default().to(self.address).input(fee_call.abi_encode().into())).await?;
        *self.fee.write().await = feeCall::abi_decode_returns(&fee_bytes)?;


        let live_balances = self.fetch_balances().await?;

        let final_balances = match self.attributes.swap_strategy {
            SwapStrategyType::AdminFee => {
                let admin_balances = self.get_admin_balances().await?;
                live_balances
                    .iter()
                    .zip(admin_balances.iter())
                    .map(|(live, admin)| live.saturating_sub(*admin))
                    .collect()
            }
            _ => {
                live_balances
            }
        };
        *self.balances.write().await = final_balances;

        if let Some(base_pool) = &self.base_pool {
            let vp_call = get_virtual_priceCall {};
            let vp_bytes = self.provider.call(TransactionRequest::default().to(base_pool.address).input(vp_call.abi_encode().into())).await?;
            *self.cached_virtual_price.write().await = Some(get_virtual_priceCall::abi_decode_returns(&vp_bytes)?);
        }

        Ok(())
    }

    pub async fn fetch_balances(&self) -> Result<Vec<U256>, ArbRsError> {
        println!("[fetch_balances] Fetching live balances for pool {}", self.address);
        let mut use_int128 = true;
        let test_call = balances_1Call { i: 0 };
        if self.provider.call(TransactionRequest::default().to(self.address).input(test_call.abi_encode().into())).await.is_err() {
            use_int128 = false;
        }

        let mut balances = Vec::with_capacity(self.attributes.n_coins);
        for i in 0..self.attributes.n_coins {
            let result_bytes = if use_int128 {
                let call = balances_1Call { i: i as i128 };
                self.provider.call(TransactionRequest::default().to(self.address).input(call.abi_encode().into())).await?
            } else {
                let call = balances_0Call { i: U256::from(i) };
                self.provider.call(TransactionRequest::default().to(self.address).input(call.abi_encode().into())).await?
            };
            let balance = if use_int128 { balances_1Call::abi_decode_returns(&result_bytes)? } else { balances_0Call::abi_decode_returns(&result_bytes)? };

            println!("[fetch_balances] balance[{}]: {}", i, balance);
            balances.push(balance);
        }
        Ok(balances)
    }

    /// Calculates the precise A value, handling the ramping logic if applicable.
    /// This is equivalent to `A_precise()` in the Vyper contract.
    pub async fn a_precise(&self, timestamp: u64) -> Result<U256, ArbRsError> {
        if let Some(ramping) = self.a_ramping_state {
            let t1 = ramping.future_a_time;
            
            if U256::from(timestamp) < t1 {
                let a0 = ramping.initial_a;
                let a1 = ramping.future_a;
                let t0 = ramping.initial_a_time;

                if a1 > a0 {
                    let time_delta = U256::from(timestamp).saturating_sub(t0);
                    let total_time = t1.saturating_sub(t0);
                    let a_delta = a1.saturating_sub(a0);

                    let intermediate = a_delta.checked_mul(time_delta)
                        .ok_or(ArbRsError::CalculationError("A ramp mul overflow".to_string()))?;
                    let ramp_amount = intermediate.checked_div(total_time)
                        .ok_or(ArbRsError::CalculationError("A ramp div by zero".to_string()))?;
                    
                    Ok(a0 + ramp_amount)
                } else {
                    let time_delta = U256::from(timestamp).saturating_sub(t0);
                    let total_time = t1.saturating_sub(t0);
                    let a_delta = a0.saturating_sub(a1);

                    let intermediate = a_delta.checked_mul(time_delta)
                        .ok_or(ArbRsError::CalculationError("A ramp mul overflow".to_string()))?;
                    let ramp_amount = intermediate.checked_div(total_time)
                        .ok_or(ArbRsError::CalculationError("A ramp div by zero".to_string()))?;
                        
                    Ok(a0 - ramp_amount)
                }
            } else {
                Ok(ramping.future_a)
            }
        } else {
            let a = *self.a.read().await;
            Ok(a.checked_mul(U256::from(100))
                .ok_or(ArbRsError::CalculationError("A_PRECISION mul overflow".to_string()))?)
        }
    }

    /// Calculates the expected amount of LP tokens for a deposit or withdrawal.
    ///
    /// This calculation accounts for slippage but does not include fees. It's primarily
    /// used to provide a `min_mint_amount` to prevent front-running attacks.
    pub async fn calc_token_amount(
        &self,
        amounts: &[U256],
        is_deposit: bool,
        // ADDED: Accept the block_number
        block_number: Option<u64>,
    ) -> Result<U256, ArbRsError> {
        let block_num = if let Some(bn) = block_number {
            bn
        } else {
            self.provider.get_block_number().await?
        };
        let block = self.provider.get_block_by_number(block_num.into()).await?
            .ok_or_else(|| ArbRsError::DataFetchError(Address::ZERO))?;
        let amp = self.a_precise(block.header.timestamp).await?;
        
        let balances = self.balances.read().await;
        let attributes = &self.attributes;

        let xp0 = math::xp(&self.attributes.rates, &balances)?;
        let d0 = math::get_d(&xp0, amp, attributes.n_coins, attributes.d_variant)?;

        if d0.is_zero() {
            return Ok(U256::ZERO);
        }

        let mut balances1 = balances.clone();
        for i in 0..self.attributes.n_coins {
            if is_deposit {
                balances1[i] = balances1[i].saturating_add(amounts[i]);
            } else {
                balances1[i] = balances1[i].checked_sub(amounts[i])
                    .ok_or(ArbRsError::CalculationError("Withdrawal amount exceeds balance".to_string()))?;
            }
        }

        let xp1 = math::xp(&self.attributes.rates, &balances1)?;
        let d1 = math::get_d(&xp1, amp, attributes.n_coins, attributes.d_variant)?;
        
        let total_supply = self.lp_token.get_total_supply(None).await?;

        let diff = if is_deposit {
            d1.saturating_sub(d0)
        } else {
            d0.saturating_sub(d1)
        };

        let lp_amount = diff.checked_mul(total_supply)
            .ok_or(ArbRsError::CalculationError("LP amount mul overflow".to_string()))?
            .checked_div(d0)
            .ok_or(ArbRsError::CalculationError("LP amount div by zero".to_string()))?;

        Ok(lp_amount)
    }

    /// Calculates the amount of a single token received upon withdrawing a
    /// specified amount of LP tokens.
    pub async fn calc_withdraw_one_coin(
        &self,
        token_amount: U256,
        i: usize,
        block_number: Option<u64>,
    ) -> Result<(U256, U256, U256), ArbRsError> {
        let block_num = if let Some(bn) = block_number { bn } else { self.provider.get_block_number().await? };
        let block = self.provider.get_block_by_number(block_num.into()).await?
            .ok_or(ArbRsError::DataFetchError(Address::ZERO))?;
        let amp = self.a_precise(block.header.timestamp).await?;

        let total_supply = self.lp_token.get_total_supply(None).await?;
        if total_supply.is_zero() {
            return Err(ArbRsError::CalculationError("LP token supply is zero".to_string()));
        }

        let balances = self.balances.read().await;
        let attributes = &self.attributes;
        let fee = *self.fee.read().await;
        let n_coins = self.attributes.n_coins;
        let precisions = &self.attributes.precision_multipliers;
        
        let xp = math::xp(&self.attributes.rates, &balances)?;
        let d0 = math::get_d(&xp, amp, n_coins, attributes.d_variant)?;

        let d1 = d0.saturating_sub(
            token_amount.checked_mul(d0)
                .ok_or(ArbRsError::CalculationError("d1 mul overflow".to_string()))?
                .checked_div(total_supply)
                .unwrap_or(U256::ZERO)
        );

        let yd_variant = pool_overrides::Y_D_VARIANT_GROUP_0.contains(&self.address);
        let new_y = math::get_y_d(amp, i, &xp, d1, n_coins, yd_variant)?;
        let dy_0 = xp[i].saturating_sub(new_y)
            .checked_div(precisions[i])
            .unwrap_or(U256::ZERO);

        let mut xp_reduced = xp.clone();
        let fee_rate = fee.checked_mul(U256::from(n_coins))
            .ok_or(ArbRsError::CalculationError("fee_rate mul overflow".to_string()))?
            .checked_div(U256::from(4 * (n_coins - 1)))
            .unwrap_or(U256::ZERO);
        
        for j in 0..n_coins {
            let ideal_balance = xp[j].checked_mul(d1)
                .ok_or(ArbRsError::CalculationError("ideal_balance mul overflow".to_string()))?
                .checked_div(d0)
                .unwrap_or(U256::ZERO);

            let difference = if j == i {
                ideal_balance.saturating_sub(new_y)
            } else {
                xp[j].saturating_sub(ideal_balance)
            };
            
            let fee_amount = fee_rate.checked_mul(difference)
                .ok_or(ArbRsError::CalculationError("fee_amount mul overflow".to_string()))?
                .checked_div(FEE_DENOMINATOR)
                .unwrap_or(U256::ZERO);
            
            xp_reduced[j] = xp_reduced[j].saturating_sub(fee_amount);
        }

        let y_after_fee = math::get_y_d(amp, i, &xp_reduced, d1, n_coins, yd_variant)?;
        let dy = xp_reduced[i].saturating_sub(y_after_fee).saturating_sub(U256::from(1))
            .checked_div(precisions[i])
            .unwrap_or(U256::ZERO);

        let final_fee = dy_0.saturating_sub(dy);

        Ok((dy, final_fee, total_supply))
    }

    /// Calculates the output amount for a swap between the underlying tokens of a metapool.
    /// This function orchestrates calls to the metapool and its base pool to simulate the full swap path.
    pub async fn calculate_dy_underlying(
        &self,
        token_in: &Token<P>,
        token_out: &Token<P>,
        dx: U256,
        block_number: Option<u64>,
    ) -> Result<U256, ArbRsError> {
        let base_pool = self.base_pool.as_ref()
            .ok_or_else(|| ArbRsError::CalculationError("This is not a metapool; cannot do an underlying swap.".to_string()))?;

        let underlying_tokens = &self.underlying_tokens;
        let i = underlying_tokens.iter().position(|t| t.address() == token_in.address())
            .ok_or_else(|| ArbRsError::CalculationError("Underlying token In not found".to_string()))?;
        let j = underlying_tokens.iter().position(|t| t.address() == token_out.address())
            .ok_or_else(|| ArbRsError::CalculationError("Underlying token Out not found".to_string()))?;

        if i > 0 && j > 0 {
            println!("[Underlying Swap] Path: Underlying -> Underlying");
            let base_i = i - 1;
            let base_j = j - 1;
            let base_token_in = &base_pool.tokens[base_i];
            let base_token_out = &base_pool.tokens[base_j];

            base_pool.calculate_tokens_out(base_token_in, base_token_out, dx, block_number).await
        } else if i > 0 && j == 0 {
            println!("[Underlying Swap] Path: Underlying -> Meta");
            let base_i = i - 1;
            let mut amounts = vec![U256::ZERO; base_pool.attributes.n_coins];
            amounts[base_i] = dx;

            let lp_token_amount = base_pool.calc_token_amount(&amounts, true, block_number).await?;
            
            let lp_token = base_pool.lp_token.as_ref();
            let meta_asset_out = token_out;

            self.calculate_tokens_out(lp_token, meta_asset_out, lp_token_amount, block_number).await
        } else if i == 0 && j > 0 {
            println!("[Underlying Swap] Path: Meta -> Underlying");
            let meta_asset_in = token_in;
            let lp_token = base_pool.lp_token.as_ref();

            let lp_token_amount = self.calculate_tokens_out(meta_asset_in, lp_token, dx, block_number).await?;

            let base_j = j - 1;
            let (dy, _fee, _supply) = base_pool.calc_withdraw_one_coin(lp_token_amount, base_j, block_number).await?;
            Ok(dy)
        } else {
            Err(ArbRsError::CalculationError("Cannot swap a token for itself.".to_string()))
        }
    }

    pub async fn get_scaled_redemption_price(
        &self,
        block_number: u64,
    ) -> Result<U256, ArbRsError> {
        if let Some(price) = self.cached_scaled_redemption_price.read().await.get(&block_number) {
            return Ok(*price);
        }

        const REDEMPTION_PRICE_SCALE: u128 = 1_000_000_000;

        let snap_addr_call = redemption_price_snapCall {};
        let snap_addr_bytes = self.provider.call(
            TransactionRequest::default()
                .to(self.address)
                .input(snap_addr_call.abi_encode().into())
        ).await?;
        let snap_contract_address = redemption_price_snapCall::abi_decode_returns(&snap_addr_bytes)?;

        let rate_call = snappedRedemptionPriceCall {};
        let rate_bytes = self.provider.call(
            TransactionRequest::default()
                .to(snap_contract_address)
                .input(rate_call.abi_encode().into())
        ).await?;
        let rate = snappedRedemptionPriceCall::abi_decode_returns(&rate_bytes)?;

        let result = rate.checked_div(U256::from(REDEMPTION_PRICE_SCALE))
            .unwrap_or_default();

        self.cached_scaled_redemption_price.write().await.insert(block_number, result);
        
        Ok(result)
    }

    /// Fetches the admin balances for each coin in the pool.
    pub async fn get_admin_balances(&self) -> Result<Vec<U256>, ArbRsError> {
        println!("[get_admin_balances] Fetching admin balances for pool {}", self.address);
        let mut use_int128 = true;
        let test_call = admin_balances_1Call { i: 0 };
        if self.provider.call(TransactionRequest::default().to(self.address).input(test_call.abi_encode().into())).await.is_err() {
            use_int128 = false;
        }

        let mut admin_balances = Vec::with_capacity(self.attributes.n_coins);
        for i in 0..self.attributes.n_coins {
            let result_bytes = if use_int128 {
                let call = admin_balances_1Call { i: i as i128 };
                self.provider.call(TransactionRequest::default().to(self.address).input(call.abi_encode().into())).await?
            } else {
                let call = admin_balances_0Call { i: U256::from(i) };
                self.provider.call(TransactionRequest::default().to(self.address).input(call.abi_encode().into())).await?
            };

            let balance = if use_int128 {
                admin_balances_1Call::abi_decode_returns(&result_bytes)?
            } else {
                admin_balances_0Call::abi_decode_returns(&result_bytes)?
            };

            println!("[get_admin_balances] admin_balance[{}]: {}", i, balance);
            admin_balances.push(balance);
        }
        Ok(admin_balances)
    }

    pub async fn fetch_balances_by_balance_of(&self) -> Result<Vec<U256>, ArbRsError> {
        let mut balances = Vec::with_capacity(self.attributes.n_coins);
        for token in self.tokens.iter() {
            let balance = token.get_balance(self.address, None).await?;
            balances.push(balance);
            println!("fetch_balances_by_balance_of: {}", balance);
        }
        Ok(balances)
    }

    pub async fn get_tricrypto_d(&self, block_number: u64) -> Result<U256, ArbRsError> {
        if let Some(d) = self.cached_tricrypto_d.read().await.get(&block_number) { return Ok(*d); }
        let call = DCall {};
        let bytes = self.provider.call(TransactionRequest::default().to(self.address).input(call.abi_encode().into())).await?;
        let d = DCall::abi_decode_returns(&bytes)?;
        self.cached_tricrypto_d.write().await.insert(block_number, d);
        Ok(d)
    }

    pub async fn get_tricrypto_gamma(&self, block_number: u64) -> Result<U256, ArbRsError> {
        if let Some(g) = self.cached_tricrypto_gamma.read().await.get(&block_number) { return Ok(*g); }
        let call = gammaCall {};
        let bytes = self.provider.call(TransactionRequest::default().to(self.address).input(call.abi_encode().into())).await?;
        let gamma = gammaCall::abi_decode_returns(&bytes)?;
        self.cached_tricrypto_gamma.write().await.insert(block_number, gamma);
        Ok(gamma)
    }

    pub async fn get_tricrypto_price_scale(&self, block_number: u64) -> Result<Vec<U256>, ArbRsError> {
        if let Some(ps) = self.cached_tricrypto_price_scale.read().await.get(&block_number) { return Ok(ps.clone()); }
        let mut price_scale = Vec::with_capacity(self.attributes.n_coins - 1);
        for i in 0..(self.attributes.n_coins - 1) {
            let call = price_scaleCall { i: U256::from(i) };
            let bytes = self.provider.call(TransactionRequest::default().to(self.address).input(call.abi_encode().into())).await?;
            let p = price_scaleCall::abi_decode_returns(&bytes)?;
            price_scale.push(p);
        }
        self.cached_tricrypto_price_scale.write().await.insert(block_number, price_scale.clone());
        Ok(price_scale)
    }

    /// Fetches the live rates from the pool's on-chain price oracle.
    pub async fn get_oracle_rates(&self, block_number: u64) -> Result<Vec<U256>, ArbRsError> {
        println!("[get_oracle_rates] Fetching for pool {}", self.address);
        if let Some(rates) = self.cached_oracle_rates.read().await.get(&block_number) {
            return Ok(rates.clone());
        }

        let call = oracle_methodCall {};
        let request = TransactionRequest::default().to(self.address).input(call.abi_encode().into());
        let bytes = self.provider.call(request).block(BlockId::from(block_number)).await?;
        let oracle_method_val = oracle_methodCall::abi_decode_returns(&bytes)?;
        
        println!("[get_oracle_rates] Found oracle_method value: {}", oracle_method_val);

        let rates = if oracle_method_val.is_zero() {
            println!("[get_oracle_rates] Using static rates.");
            self.attributes.rates.clone()
        } else {
            let oracle_address = Address::from_slice(&oracle_method_val.to_be_bytes::<32>()[12..]);

            // Extract the calldata (everything except the last 20 bytes)
            // We need to be careful with endianness. U256 is little-endian.
            let mut calldata_bytes = oracle_method_val.to_be_bytes::<32>();
            // Zero out the address part to isolate the calldata
            calldata_bytes[12..].iter_mut().for_each(|byte| *byte = 0);
            let calldata = U256::from_be_bytes(calldata_bytes);

            println!("[get_oracle_rates] Calling oracle {} with calldata {}", oracle_address, calldata);

            let oracle_request = TransactionRequest::default().to(oracle_address).input(calldata.to_be_bytes_vec().into());
            let oracle_result_bytes = self.provider.call(oracle_request).block(BlockId::from(block_number)).await?;

            let oracle_price = U256::from_be_slice(&oracle_result_bytes);

            println!("[get_oracle_rates] Oracle returned price: {}", oracle_price);

            vec![
                self.attributes.rates[0],
                self.attributes.rates[1].checked_mul(oracle_price)
                    .ok_or_else(|| ArbRsError::CalculationError("Oracle rate mul overflow".to_string()))?
                    .checked_div(PRECISION)
                    .ok_or_else(|| ArbRsError::CalculationError("Oracle rate div underflow".to_string()))?
            ]
        };

        self.cached_oracle_rates.write().await.insert(block_number, rates.clone());
        Ok(rates)
    }
}

impl<P: ?Sized + Provider> std::fmt::Debug for CurveStableswapPool<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CurveStableswapPool").field("address", &self.address).finish_non_exhaustive()
    }
}
