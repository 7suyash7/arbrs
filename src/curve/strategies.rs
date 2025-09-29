use crate::TokenLike;
use crate::curve::constants::{FEE_DENOMINATOR, PRECISION};
use crate::curve::pool::{
    CurveStableswapPool, accrualBlockNumberCall, exchangeRateStoredCall, getExchangeRateCall,
    ratioCall, supplyRatePerBlockCall,
};
use crate::curve::pool_overrides::{DVariant, Y_VARIANT_GROUP_0, Y_VARIANT_GROUP_1};
use crate::curve::tricrypto_math::TEN_POW_18;
use crate::curve::{math, tricrypto_math};
use crate::errors::ArbRsError;
use alloy_primitives::{Address, U256, address};
use alloy_provider::Provider;
use alloy_rpc_types::TransactionRequest;
use alloy_sol_types::SolCall;
use async_trait::async_trait;

// Metapool addresses
const STETH_USDC_METAPOOL: Address = address!("C61557C5d177bd7DC889A3b621eEC333e168f68A");
const RETH_ETH_METAPOOL: Address = address!("618788357D0EBd8A37e763ADab3bc575D54c2C7d");
const COMPOUND_POOL_ADDRESS: Address = address!("A2B47E3D5c44877cca798226B7B8118F9BFb7A56");
const AAVE_POOL_ADDRESS: Address = address!("52EA46506B9CC5Ef470C5bf89f17Dc28bB35D85C");
const ANKRETH_POOL: Address = address!("A96A65c051bF88B4095Ee1f2451C2A9d43F53Ae2");
const IRON_BANK_POOL: Address = address!("2dded6Da1BF5DBdF597C45fcFaa3194e53EcfeAF");
const RETH_POOL: Address = address!("F9440930043eb3997fc70e1339dBb11F341de7A8");

// These addresses use a slightly different final `dy` calculation
const LENDING_GROUP_A: &[Address] = &[
    COMPOUND_POOL_ADDRESS,
    AAVE_POOL_ADDRESS,
    RETH_POOL,
    address!("A5407eAE9Ba41422680e2e00537571bcC53efBfD"), // sUSD
    address!("45F783CCE6B7FF23B2ab2D70e416cdb7D6055f51"), // bUSD/y
    address!("79a8C46DeA5aDa233ABaFFD40F3A0A2B1e5A4F27"), // y
    address!("A96A65c051bF88B4095Ee1f2451C2A9d43F53Ae2"), // ankrETH Pool
];
const LENDING_GROUP_B: &[Address] = &[
    address!("A96A65c051bF88B4095Ee1f2451C2A9d43F53Ae2"), // aETH
];

/// A struct to encapsulate all the necessary parameters for a swap calculation.
/// This is passed to the `calculate_dy` method of a `SwapStrategy`.
#[derive(Debug)]
pub struct SwapParams<'a, P: Provider + Send + Sync + 'static + ?Sized> {
    /// The index of the coin being sent.
    pub i: usize,
    /// The index of the coin being received.
    pub j: usize,
    /// The amount of the input coin being sent.
    pub dx: U256,
    /// A reference to the pool itself, providing access to its state (balances, attributes, provider).
    pub pool: &'a CurveStableswapPool<P>,
    /// The timestamp of the block for which the calculation is being performed.
    pub block_timestamp: u64,
    /// The block number for which the calculation is being performed.
    pub block_number: u64,
}

/// A common interface for all Curve swap calculation strategies.
#[async_trait]
pub trait SwapStrategy<P: Provider + Send + Sync + 'static + ?Sized> {
    /// Calculates the output amount `dy` for a given input amount `dx`.
    async fn calculate_dy(&self, params: &SwapParams<'_, P>) -> Result<U256, ArbRsError>;

    /// Calculates the required input amount `dx` for a desired output amount `dy`.
    async fn calculate_dx(&self, params: &SwapParams<'_, P>, dy: U256) -> Result<U256, ArbRsError>;
}

/// Strategy for standard Curve V1 pools.
/// Logic: xp -> x -> y -> dy -> fee -> unscale by rate
#[derive(Debug, Default)]
pub struct DefaultStrategy;

#[async_trait]
impl<P: Provider + Send + Sync + 'static + ?Sized> SwapStrategy<P> for DefaultStrategy {
    async fn calculate_dy(&self, params: &SwapParams<'_, P>) -> Result<U256, ArbRsError> {
        let SwapParams {
            i,
            j,
            dx,
            pool,
            block_timestamp,
            block_number,
        } = params;
        let (i, j, dx, block_timestamp, _block_number) =
            (*i, *j, *dx, *block_timestamp, *block_number);

        let balances = pool.balances.read().await;
        let attributes = &pool.attributes;
        let fee = *pool.fee.read().await;

        let amp = pool.a_precise(block_timestamp).await?;
        let xp = math::xp(&attributes.rates, &balances)?;

        let dx_scaled = dx
            .checked_mul(attributes.rates[i])
            .ok_or_else(|| ArbRsError::CalculationError("dx_scaled mul overflow".to_string()))?
            .checked_div(PRECISION)
            .ok_or_else(|| ArbRsError::CalculationError("dx_scaled div underflow".to_string()))?;

        let x = xp[i]
            .checked_add(dx_scaled)
            .ok_or_else(|| ArbRsError::CalculationError("x add overflow".to_string()))?;

        let is_y0 = Y_VARIANT_GROUP_0.contains(&pool.address);
        let is_y1 = Y_VARIANT_GROUP_1.contains(&pool.address);
        let y = math::get_y(
            i,
            j,
            x,
            &xp,
            amp,
            attributes.n_coins,
            attributes.d_variant,
            is_y0,
            is_y1,
        )?;

        let dy = xp[j].saturating_sub(y).saturating_sub(U256::from(1));

        let fee_amount = dy
            .checked_mul(fee)
            .ok_or_else(|| ArbRsError::CalculationError("fee_amount mul overflow".to_string()))?
            .checked_div(FEE_DENOMINATOR)
            .ok_or_else(|| ArbRsError::CalculationError("fee_amount div underflow".to_string()))?;

        let dy_after_fee = dy.saturating_sub(fee_amount);

        let rate_j = attributes.rates[j];
        if rate_j.is_zero() {
            return Err(ArbRsError::CalculationError(
                "Output token rate is zero".to_string(),
            ));
        }

        let final_dy = dy_after_fee
            .checked_mul(PRECISION)
            .ok_or_else(|| ArbRsError::CalculationError("final_dy mul overflow".to_string()))?
            .checked_div(rate_j)
            .ok_or_else(|| ArbRsError::CalculationError("final_dy div underflow".to_string()))?;

        Ok(final_dy)
    }

    async fn calculate_dx(&self, params: &SwapParams<'_, P>, dy: U256) -> Result<U256, ArbRsError> {
        let SwapParams { i, j, pool, .. } = params;
        let (i, j) = (*i, *j);

        let balances = pool.balances.read().await;
        let attributes = &pool.attributes;
        let fee = *pool.fee.read().await;
        let amp = pool.a_precise(params.block_timestamp).await?;
        let rates = &attributes.rates;
        let xp = math::xp(rates, &*balances)?;

        let dy_plus_fee = dy
            .checked_mul(FEE_DENOMINATOR)
            .ok_or_else(|| ArbRsError::CalculationError("dy_plus_fee mul overflow".to_string()))?
            .checked_div(FEE_DENOMINATOR.saturating_sub(fee))
            .ok_or_else(|| ArbRsError::CalculationError("dy_plus_fee div underflow".to_string()))?;

        let dy_scaled = dy_plus_fee
            .checked_mul(rates[j])
            .ok_or_else(|| ArbRsError::CalculationError("dy_scaled mul overflow".to_string()))?
            .checked_div(PRECISION)
            .ok_or_else(|| ArbRsError::CalculationError("dy_scaled div underflow".to_string()))?;

        let y = xp[j]
            .checked_sub(dy_scaled)
            .ok_or_else(|| ArbRsError::CalculationError("y sub underflow".to_string()))?;

        let is_y0 = Y_VARIANT_GROUP_0.contains(&pool.address);
        let is_y1 = Y_VARIANT_GROUP_1.contains(&pool.address);
        let x = math::get_y(
            j,
            i,
            y,
            &xp,
            amp,
            attributes.n_coins,
            attributes.d_variant,
            is_y0,
            is_y1,
        )?;

        let dx_scaled = x
            .checked_sub(xp[i])
            .ok_or_else(|| ArbRsError::CalculationError("dx_scaled sub underflow".to_string()))?;

        let rate_i = rates[i];
        if rate_i.is_zero() {
            return Err(ArbRsError::CalculationError(
                "Input token rate is zero".to_string(),
            ));
        }

        Ok(dx_scaled
            .checked_mul(PRECISION)
            .ok_or_else(|| ArbRsError::CalculationError("final_dx mul overflow".to_string()))?
            .checked_div(rate_i)
            .ok_or_else(|| ArbRsError::CalculationError("final_dx div underflow".to_string()))?
            .saturating_add(U256::from(1)))
    }
}

/// Strategy for metapools, which require special rate handling for the base pool LP token.
#[derive(Debug, Default)]
pub struct MetapoolStrategy;

#[async_trait]
impl<P: Provider + Send + Sync + 'static + ?Sized> SwapStrategy<P> for MetapoolStrategy {
    async fn calculate_dy(&self, params: &SwapParams<'_, P>) -> Result<U256, ArbRsError> {
        let SwapParams {
            i,
            j,
            dx,
            pool,
            block_timestamp,
            block_number,
        } = params;
        let (i, j, dx, block_timestamp, _block_number) =
            (*i, *j, *dx, *block_timestamp, *block_number);

        let balances = pool.balances.read().await;
        let attributes = &pool.attributes;
        let fee = *pool.fee.read().await;
        let amp = pool.a_precise(block_timestamp).await?;

        if attributes.n_coins != 2 {
            return Err(ArbRsError::CalculationError(
                "Metapool strategy only supports 2-coin pools".to_string(),
            ));
        }

        let virtual_price = pool.cached_virtual_price.read().await.ok_or_else(|| {
            ArbRsError::CalculationError("Metapool virtual price not available".to_string())
        })?;

        let block_number = pool.provider.get_block_number().await?;

        let rates = match pool.address {
            STETH_USDC_METAPOOL => vec![PRECISION, virtual_price],
            RETH_ETH_METAPOOL => vec![
                pool.get_scaled_redemption_price(block_number).await?,
                virtual_price,
            ],
            _ => vec![attributes.rates[0], virtual_price],
        };

        let xp = math::xp(&rates, &balances)?;

        let dx_scaled = dx
            .checked_mul(rates[i])
            .ok_or_else(|| ArbRsError::CalculationError("dx_scaled mul overflow".to_string()))?
            .checked_div(PRECISION)
            .ok_or_else(|| ArbRsError::CalculationError("dx_scaled div underflow".to_string()))?;

        let x = xp[i]
            .checked_add(dx_scaled)
            .ok_or_else(|| ArbRsError::CalculationError("x add overflow".to_string()))?;

        let is_y0 = Y_VARIANT_GROUP_0.contains(&pool.address);
        let is_y1 = Y_VARIANT_GROUP_1.contains(&pool.address);
        let y = math::get_y(
            i,
            j,
            x,
            &xp,
            amp,
            attributes.n_coins,
            attributes.d_variant,
            is_y0,
            is_y1,
        )?;

        let dy = xp[j].saturating_sub(y).saturating_sub(U256::from(1));

        let fee_amount = dy
            .checked_mul(fee)
            .ok_or_else(|| ArbRsError::CalculationError("fee_amount mul overflow".to_string()))?
            .checked_div(FEE_DENOMINATOR)
            .ok_or_else(|| ArbRsError::CalculationError("fee_amount div underflow".to_string()))?;

        let dy_after_fee = dy.saturating_sub(fee_amount);

        let rate_j = rates[j];
        if rate_j.is_zero() {
            return Err(ArbRsError::CalculationError(
                "Output token rate is zero".to_string(),
            ));
        }

        let final_dy = dy_after_fee
            .checked_mul(PRECISION)
            .ok_or_else(|| ArbRsError::CalculationError("final_dy mul overflow".to_string()))?
            .checked_div(rate_j)
            .ok_or_else(|| ArbRsError::CalculationError("final_dy div underflow".to_string()))?;

        Ok(final_dy)
    }

    async fn calculate_dx(&self, params: &SwapParams<'_, P>, dy: U256) -> Result<U256, ArbRsError> {
        let balances = params.pool.balances.read().await;
        let attributes = &params.pool.attributes;
        let fee = *params.pool.fee.read().await;
        let amp = params.pool.a_precise(params.block_timestamp).await?;

        if attributes.n_coins != 2 {
            return Err(ArbRsError::CalculationError(
                "Metapool strategy only supports 2-coin pools".to_string(),
            ));
        }

        let virtual_price = params
            .pool
            .cached_virtual_price
            .read()
            .await
            .ok_or_else(|| {
                ArbRsError::CalculationError("Metapool virtual price not available".to_string())
            })?;
        let rates = match params.pool.address {
            STETH_USDC_METAPOOL => vec![PRECISION, virtual_price],
            RETH_ETH_METAPOOL => vec![
                params
                    .pool
                    .get_scaled_redemption_price(params.block_number)
                    .await?,
                virtual_price,
            ],
            _ => vec![attributes.rates[0], virtual_price],
        };
        let xp = math::xp(&rates, &*balances)?;

        let dy_plus_fee = dy
            .checked_mul(FEE_DENOMINATOR)
            .ok_or_else(|| ArbRsError::CalculationError("dy_plus_fee mul overflow".to_string()))?
            .checked_div(FEE_DENOMINATOR.saturating_sub(fee))
            .ok_or_else(|| ArbRsError::CalculationError("dy_plus_fee div underflow".to_string()))?;

        let dy_scaled = dy_plus_fee
            .checked_mul(rates[params.j])
            .ok_or_else(|| ArbRsError::CalculationError("dy_scaled mul overflow".to_string()))?
            .checked_div(PRECISION)
            .ok_or_else(|| ArbRsError::CalculationError("dy_scaled div underflow".to_string()))?;

        let y = xp[params.j]
            .checked_sub(dy_scaled)
            .ok_or_else(|| ArbRsError::CalculationError("y sub underflow".to_string()))?;

        let is_y0 = Y_VARIANT_GROUP_0.contains(&params.pool.address);
        let is_y1 = Y_VARIANT_GROUP_1.contains(&params.pool.address);
        let x = math::get_y(
            params.j,
            params.i,
            y,
            &xp,
            amp,
            attributes.n_coins,
            attributes.d_variant,
            is_y0,
            is_y1,
        )?;

        let dx_scaled = x
            .checked_sub(xp[params.i])
            .ok_or_else(|| ArbRsError::CalculationError("dx_scaled sub underflow".to_string()))?;

        let rate_i = rates[params.i];
        if rate_i.is_zero() {
            return Err(ArbRsError::CalculationError(
                "Input token rate is zero".to_string(),
            ));
        }

        Ok(dx_scaled
            .checked_mul(PRECISION)
            .ok_or_else(|| ArbRsError::CalculationError("final_dx mul overflow".to_string()))?
            .checked_div(rate_i)
            .ok_or_else(|| ArbRsError::CalculationError("final_dx div underflow".to_string()))?
            .saturating_add(U256::from(1)))
    }
}

/// Strategy for pools with lending tokens (aTokens, cTokens, yTokens) that require fetching live rates.
#[derive(Debug, Default)]
pub struct LendingStrategy;

#[async_trait]
impl<P: Provider + Send + Sync + 'static + ?Sized> SwapStrategy<P> for LendingStrategy {
    async fn calculate_dy(&self, params: &SwapParams<'_, P>) -> Result<U256, ArbRsError> {
        let SwapParams {
            i,
            j,
            dx,
            pool,
            block_timestamp,
            block_number,
        } = params;
        let (i, j, dx, block_number, block_timestamp) =
            (*i, *j, *dx, *block_number, *block_timestamp);

        let attributes = &pool.attributes;
        let provider = &pool.provider;
        let fee = *pool.fee.read().await;
        let balances = pool.balances.read().await;
        let amp = pool.a_precise(block_timestamp).await?;

        let rates = if pool.address == ANKRETH_POOL {
            let ankr_token = &pool.tokens[1];
            let ratio_call = ratioCall {};
            let rate_bytes = provider
                .call(
                    TransactionRequest::default()
                        .to(ankr_token.address())
                        .input(ratio_call.abi_encode().into()),
                )
                .await?;
            let ratio = ratioCall::abi_decode_returns(&rate_bytes)?;

            let ankr_rate = PRECISION
                .checked_mul(PRECISION)
                .ok_or_else(|| ArbRsError::CalculationError("ankr_rate mul overflow".to_string()))?
                .checked_div(ratio)
                .ok_or_else(|| {
                    ArbRsError::CalculationError("ankr_rate div underflow".to_string())
                })?;
            vec![PRECISION, ankr_rate]
        } else if pool.address == RETH_POOL {
            let reth_token = &pool.tokens[1];
            let rate_call = getExchangeRateCall {};
            let rate_bytes = provider
                .call(
                    TransactionRequest::default()
                        .to(reth_token.address())
                        .input(rate_call.abi_encode().into()),
                )
                .await?;
            let reth_rate = getExchangeRateCall::abi_decode_returns(&rate_bytes)?;
            vec![PRECISION, reth_rate]
        } else {
            let mut calculated_rates = Vec::with_capacity(attributes.n_coins);
            for (idx, token) in pool.tokens.iter().enumerate() {
                let final_rate_result = if attributes.use_lending[idx] {
                    if pool.address == COMPOUND_POOL_ADDRESS
                        || pool.address == AAVE_POOL_ADDRESS
                        || pool.address == IRON_BANK_POOL
                    {
                        let rate_call = exchangeRateStoredCall {};
                        let rate_bytes = provider
                            .call(
                                TransactionRequest::default()
                                    .to(token.address())
                                    .input(rate_call.abi_encode().into()),
                            )
                            .await?;
                        let mut rate = exchangeRateStoredCall::abi_decode_returns(&rate_bytes)?;

                        let supply_rate_call = supplyRatePerBlockCall {};
                        let sr_bytes = provider
                            .call(
                                TransactionRequest::default()
                                    .to(token.address())
                                    .input(supply_rate_call.abi_encode().into()),
                            )
                            .await?;
                        let supply_rate = supplyRatePerBlockCall::abi_decode_returns(&sr_bytes)?;

                        let accrual_block_call = accrualBlockNumberCall {};
                        let ab_bytes = provider
                            .call(
                                TransactionRequest::default()
                                    .to(token.address())
                                    .input(accrual_block_call.abi_encode().into()),
                            )
                            .await?;
                        let old_block = accrualBlockNumberCall::abi_decode_returns(&ab_bytes)?;

                        if U256::from(block_number) > old_block {
                            let interest = rate
                                .checked_mul(supply_rate)
                                .ok_or_else(|| {
                                    ArbRsError::CalculationError(
                                        "cToken interest mul1 overflow".to_string(),
                                    )
                                })?
                                .checked_mul(U256::from(block_number) - old_block)
                                .ok_or_else(|| {
                                    ArbRsError::CalculationError(
                                        "cToken interest mul2 overflow".to_string(),
                                    )
                                })?
                                .checked_div(PRECISION)
                                .ok_or_else(|| {
                                    ArbRsError::CalculationError(
                                        "cToken interest div underflow".to_string(),
                                    )
                                })?;
                            rate += interest;
                        }
                        rate.checked_mul(attributes.precision_multipliers[idx])
                            .ok_or_else(|| {
                                ArbRsError::CalculationError(
                                    "cToken final rate mul overflow".to_string(),
                                )
                            })
                    } else {
                        let rate_call = exchangeRateStoredCall {};
                        let rate_bytes = provider
                            .call(
                                TransactionRequest::default()
                                    .to(token.address())
                                    .input(rate_call.abi_encode().into()),
                            )
                            .await?;
                        let stored_rate = exchangeRateStoredCall::abi_decode_returns(&rate_bytes)?;
                        stored_rate
                            .checked_mul(attributes.precision_multipliers[idx])
                            .ok_or_else(|| {
                                ArbRsError::CalculationError(
                                    "Lending rate mul overflow".to_string(),
                                )
                            })
                    }
                } else {
                    Ok(attributes.rates[idx])
                };
                calculated_rates.push(final_rate_result?);
            }
            calculated_rates
        };

        let xp = math::xp(&rates, &*balances)?;
        let dx_scaled = dx
            .checked_mul(rates[i])
            .ok_or_else(|| ArbRsError::CalculationError("dx_scaled mul overflow".to_string()))?
            .checked_div(PRECISION)
            .ok_or_else(|| ArbRsError::CalculationError("dx_scaled div underflow".to_string()))?;
        let x = xp[i]
            .checked_add(dx_scaled)
            .ok_or_else(|| ArbRsError::CalculationError("x add overflow".to_string()))?;

        let is_y0 = Y_VARIANT_GROUP_0.contains(&pool.address);
        let is_y1 = Y_VARIANT_GROUP_1.contains(&pool.address);
        let y = math::get_y(
            i,
            j,
            x,
            &xp,
            amp,
            attributes.n_coins,
            attributes.d_variant,
            is_y0,
            is_y1,
        )?;
        let dy_raw = xp[j].saturating_sub(y);

        if LENDING_GROUP_A.contains(&pool.address) {
            let fee_amount = dy_raw
                .checked_mul(fee)
                .ok_or_else(|| ArbRsError::CalculationError("fee_amount mul overflow".to_string()))?
                .checked_div(FEE_DENOMINATOR)
                .ok_or_else(|| {
                    ArbRsError::CalculationError("fee_amount div underflow".to_string())
                })?;
            let dy_after_fee = dy_raw.saturating_sub(fee_amount);
            Ok(dy_after_fee
                .checked_mul(PRECISION)
                .ok_or_else(|| ArbRsError::CalculationError("final_dy mul overflow".to_string()))?
                .checked_div(rates[j])
                .ok_or_else(|| {
                    ArbRsError::CalculationError("final_dy div underflow".to_string())
                })?)
        } else if LENDING_GROUP_B.contains(&pool.address) {
            let fee_amount = dy_raw
                .checked_mul(fee)
                .ok_or_else(|| ArbRsError::CalculationError("fee_amount mul overflow".to_string()))?
                .checked_div(FEE_DENOMINATOR)
                .ok_or_else(|| {
                    ArbRsError::CalculationError("fee_amount div underflow".to_string())
                })?;
            Ok(dy_raw.saturating_sub(fee_amount))
        } else {
            let dy_with_margin = dy_raw.saturating_sub(U256::from(1));
            let final_dy = dy_with_margin
                .checked_mul(PRECISION)
                .ok_or_else(|| ArbRsError::CalculationError("final_dy mul overflow".to_string()))?
                .checked_div(rates[j])
                .ok_or_else(|| {
                    ArbRsError::CalculationError("final_dy div underflow".to_string())
                })?;
            let fee_amount = final_dy
                .checked_mul(fee)
                .ok_or_else(|| ArbRsError::CalculationError("fee_amount mul overflow".to_string()))?
                .checked_div(FEE_DENOMINATOR)
                .ok_or_else(|| {
                    ArbRsError::CalculationError("fee_amount div underflow".to_string())
                })?;
            Ok(final_dy.saturating_sub(fee_amount))
        }
    }

    async fn calculate_dx(&self, params: &SwapParams<'_, P>, dy: U256) -> Result<U256, ArbRsError> {
        let balances = params.pool.balances.read().await;
        let attributes = &params.pool.attributes;
        let fee = *params.pool.fee.read().await;
        let provider = &params.pool.provider;
        let block_number = params.block_number;

        let mut rates = Vec::with_capacity(attributes.n_coins);
        for (idx, token) in params.pool.tokens.iter().enumerate() {
            let final_rate = if attributes.use_lending[idx] {
                if params.pool.address == COMPOUND_POOL_ADDRESS
                    || params.pool.address == AAVE_POOL_ADDRESS
                {
                    let rate_call = exchangeRateStoredCall {};
                    let rate_bytes = provider
                        .call(
                            TransactionRequest::default()
                                .to(token.address())
                                .input(rate_call.abi_encode().into()),
                        )
                        .await?;
                    let mut rate = exchangeRateStoredCall::abi_decode_returns(&rate_bytes)?;

                    let supply_rate_call = supplyRatePerBlockCall {};
                    let sr_bytes = provider
                        .call(
                            TransactionRequest::default()
                                .to(token.address())
                                .input(supply_rate_call.abi_encode().into()),
                        )
                        .await?;
                    let supply_rate = supplyRatePerBlockCall::abi_decode_returns(&sr_bytes)?;

                    let accrual_block_call = accrualBlockNumberCall {};
                    let ab_bytes = provider
                        .call(
                            TransactionRequest::default()
                                .to(token.address())
                                .input(accrual_block_call.abi_encode().into()),
                        )
                        .await?;
                    let old_block = accrualBlockNumberCall::abi_decode_returns(&ab_bytes)?;

                    if U256::from(block_number) > old_block {
                        let interest = rate
                            .checked_mul(supply_rate)
                            .ok_or_else(|| {
                                ArbRsError::CalculationError(
                                    "cToken interest mul1 overflow".to_string(),
                                )
                            })?
                            .checked_mul(U256::from(block_number) - old_block)
                            .ok_or_else(|| {
                                ArbRsError::CalculationError(
                                    "cToken interest mul2 overflow".to_string(),
                                )
                            })?
                            .checked_div(PRECISION)
                            .ok_or_else(|| {
                                ArbRsError::CalculationError(
                                    "cToken interest div underflow".to_string(),
                                )
                            })?;
                        rate += interest;
                    }
                    rate.checked_mul(attributes.precision_multipliers[idx])
                        .ok_or_else(|| {
                            ArbRsError::CalculationError(
                                "cToken final rate mul overflow".to_string(),
                            )
                        })?
                } else {
                    let rate_call = exchangeRateStoredCall {};
                    let rate_bytes = provider
                        .call(
                            TransactionRequest::default()
                                .to(token.address())
                                .input(rate_call.abi_encode().into()),
                        )
                        .await?;
                    let stored_rate = exchangeRateStoredCall::abi_decode_returns(&rate_bytes)?;
                    stored_rate
                        .checked_mul(attributes.precision_multipliers[idx])
                        .ok_or_else(|| {
                            ArbRsError::CalculationError("Lending rate mul overflow".to_string())
                        })?
                }
            } else {
                attributes.rates[idx]
            };
            rates.push(final_rate);
        }

        let amp = params.pool.a_precise(params.block_timestamp).await?;
        let xp = math::xp(&rates, &*balances)?;

        let dy_plus_fee = dy
            .checked_mul(FEE_DENOMINATOR)
            .ok_or_else(|| ArbRsError::CalculationError("dy_plus_fee mul overflow".to_string()))?
            .checked_div(FEE_DENOMINATOR.saturating_sub(fee))
            .ok_or_else(|| ArbRsError::CalculationError("dy_plus_fee div underflow".to_string()))?;

        let dy_scaled = dy_plus_fee
            .checked_mul(rates[params.j])
            .ok_or_else(|| ArbRsError::CalculationError("dy_scaled mul overflow".to_string()))?
            .checked_div(PRECISION)
            .ok_or_else(|| ArbRsError::CalculationError("dy_scaled div underflow".to_string()))?;

        let y = xp[params.j]
            .checked_sub(dy_scaled)
            .ok_or_else(|| ArbRsError::CalculationError("y sub underflow".to_string()))?;

        let is_y0 = Y_VARIANT_GROUP_0.contains(&params.pool.address);
        let is_y1 = Y_VARIANT_GROUP_1.contains(&params.pool.address);
        let x = math::get_y(
            params.j,
            params.i,
            y,
            &xp,
            amp,
            attributes.n_coins,
            attributes.d_variant,
            is_y0,
            is_y1,
        )?;

        let dx_scaled = x
            .checked_sub(xp[params.i])
            .ok_or_else(|| ArbRsError::CalculationError("dx_scaled sub underflow".to_string()))?;

        let rate_i = rates[params.i];
        if rate_i.is_zero() {
            return Err(ArbRsError::CalculationError(
                "Input token rate is zero".to_string(),
            ));
        }

        Ok(dx_scaled
            .checked_mul(PRECISION)
            .ok_or_else(|| ArbRsError::CalculationError("final_dx mul overflow".to_string()))?
            .checked_div(rate_i)
            .ok_or_else(|| ArbRsError::CalculationError("final_dx div underflow".to_string()))?
            .saturating_add(U256::from(1)))
    }
}

/// Strategy for pools that do not use rate scaling. `xp` balances are the same as token balances.
#[derive(Debug, Default)]
pub struct UnscaledStrategy;

#[async_trait]
impl<P: Provider + Send + Sync + 'static + ?Sized> SwapStrategy<P> for UnscaledStrategy {
    async fn calculate_dy(&self, params: &SwapParams<'_, P>) -> Result<U256, ArbRsError> {
        let SwapParams {
            i,
            j,
            dx,
            pool,
            block_timestamp,
            block_number,
        } = params;
        let (i, j, dx, block_timestamp, _block_number) =
            (*i, *j, *dx, *block_timestamp, *block_number);

        let balances = pool.balances.read().await;
        let attributes = &pool.attributes;
        let fee = *pool.fee.read().await;

        let amp = pool.a_precise(block_timestamp).await?;

        let xp = balances.clone();

        let x = xp[i]
            .checked_add(dx)
            .ok_or_else(|| ArbRsError::CalculationError("x add overflow".to_string()))?;

        let is_y0 = Y_VARIANT_GROUP_0.contains(&pool.address);
        let is_y1 = Y_VARIANT_GROUP_1.contains(&pool.address);
        let y = math::get_y(
            i,
            j,
            x,
            &xp,
            amp,
            attributes.n_coins,
            attributes.d_variant,
            is_y0,
            is_y1,
        )?;

        let dy = xp[j].saturating_sub(y).saturating_sub(U256::from(1));

        let fee_amount = dy
            .checked_mul(fee)
            .ok_or_else(|| ArbRsError::CalculationError("fee_amount mul overflow".to_string()))?
            .checked_div(FEE_DENOMINATOR)
            .ok_or_else(|| ArbRsError::CalculationError("fee_amount div underflow".to_string()))?;

        let final_dy = dy.saturating_sub(fee_amount);

        Ok(final_dy)
    }

    async fn calculate_dx(&self, params: &SwapParams<'_, P>, dy: U256) -> Result<U256, ArbRsError> {
        let balances = params.pool.balances.read().await;
        let attributes = &params.pool.attributes;
        let fee = *params.pool.fee.read().await;
        let amp = params.pool.a_precise(params.block_timestamp).await?;

        let xp = balances.clone();

        let dy_plus_fee = dy
            .checked_mul(FEE_DENOMINATOR)
            .ok_or_else(|| ArbRsError::CalculationError("dy_plus_fee mul overflow".to_string()))?
            .checked_div(FEE_DENOMINATOR.saturating_sub(fee))
            .ok_or_else(|| ArbRsError::CalculationError("dy_plus_fee div underflow".to_string()))?;

        let y = xp[params.j]
            .checked_sub(dy_plus_fee)
            .ok_or_else(|| ArbRsError::CalculationError("y sub underflow".to_string()))?;

        let is_y0 = Y_VARIANT_GROUP_0.contains(&params.pool.address);
        let is_y1 = Y_VARIANT_GROUP_1.contains(&params.pool.address);
        let x = math::get_y(
            params.j,
            params.i,
            y,
            &xp,
            amp,
            attributes.n_coins,
            attributes.d_variant,
            is_y0,
            is_y1,
        )?;

        Ok(x.checked_sub(xp[params.i])
            .ok_or_else(|| ArbRsError::CalculationError("dx sub underflow".to_string()))?
            .saturating_add(U256::from(1)))
    }
}

/// Strategy for pools with dynamic fees based on an off-peg multiplier (e.g., stETH, MIM).
#[derive(Debug, Default)]
pub struct DynamicFeeStrategy;

#[async_trait]
impl<P: Provider + Send + Sync + 'static + ?Sized> SwapStrategy<P> for DynamicFeeStrategy {
    async fn calculate_dy(&self, params: &SwapParams<'_, P>) -> Result<U256, ArbRsError> {
        let SwapParams {
            i,
            j,
            dx,
            pool,
            block_timestamp,
            ..
        } = params;
        let (i, j, dx) = (*i, *j, *dx);

        let balances = pool.balances.read().await;
        let attributes = &pool.attributes;
        let fee = *pool.fee.read().await;

        let rates = &attributes.rates;
        let xp = math::xp(rates, &balances)?;

        let dx_scaled = dx
            .checked_mul(rates[i])
            .ok_or_else(|| ArbRsError::CalculationError("dx_scaled mul overflow".to_string()))?
            .checked_div(PRECISION)
            .ok_or_else(|| ArbRsError::CalculationError("dx_scaled div underflow".to_string()))?;

        let x = xp[i]
            .checked_add(dx_scaled)
            .ok_or_else(|| ArbRsError::CalculationError("x add overflow".to_string()))?;

        let amp = pool.a_precise(*block_timestamp).await?;
        let is_y0 = Y_VARIANT_GROUP_0.contains(&pool.address);
        let is_y1 = Y_VARIANT_GROUP_1.contains(&pool.address);

        let y = math::get_y(
            i,
            j,
            x,
            &xp,
            amp,
            attributes.n_coins,
            attributes.d_variant,
            is_y0,
            is_y1,
        )?;

        let dy = xp[j].saturating_sub(y).saturating_sub(U256::from(1));

        let fee_amount = dy
            .checked_mul(fee)
            .ok_or_else(|| ArbRsError::CalculationError("fee_amount mul overflow".to_string()))?
            .checked_div(FEE_DENOMINATOR)
            .ok_or_else(|| ArbRsError::CalculationError("fee_amount div underflow".to_string()))?;

        let dy_after_fee = dy.saturating_sub(fee_amount);

        let rate_j = rates[j];
        if rate_j.is_zero() {
            return Err(ArbRsError::CalculationError(
                "Output token rate is zero".to_string(),
            ));
        }

        let final_dy = dy_after_fee
            .checked_mul(PRECISION)
            .ok_or_else(|| ArbRsError::CalculationError("final_dy mul overflow".to_string()))?
            .checked_div(rate_j)
            .ok_or_else(|| ArbRsError::CalculationError("final_dy div underflow".to_string()))?;

        Ok(final_dy)
    }

    async fn calculate_dx(&self, params: &SwapParams<'_, P>, dy: U256) -> Result<U256, ArbRsError> {
        let balances = params.pool.balances.read().await;
        let attributes = &params.pool.attributes;
        let fee = *params.pool.fee.read().await;
        let amp = params.pool.a_precise(params.block_timestamp).await?;

        let xp = balances.clone();

        let dy_plus_fee = dy
            .checked_mul(FEE_DENOMINATOR)
            .ok_or_else(|| ArbRsError::CalculationError("dy_plus_fee mul overflow".to_string()))?
            .checked_div(FEE_DENOMINATOR.saturating_sub(fee))
            .ok_or_else(|| ArbRsError::CalculationError("dy_plus_fee div underflow".to_string()))?;

        let y = xp[params.j]
            .checked_sub(dy_plus_fee)
            .ok_or_else(|| ArbRsError::CalculationError("y sub underflow".to_string()))?;

        let is_y0 = Y_VARIANT_GROUP_0.contains(&params.pool.address);
        let is_y1 = Y_VARIANT_GROUP_1.contains(&params.pool.address);
        let x = math::get_y(
            params.j,
            params.i,
            y,
            &xp,
            amp,
            attributes.n_coins,
            attributes.d_variant,
            is_y0,
            is_y1,
        )?;

        Ok(x.checked_sub(xp[params.i])
            .ok_or_else(|| ArbRsError::CalculationError("dx sub underflow".to_string()))?
            .saturating_add(U256::from(1)))
    }
}

/// Strategy for the unique Tricrypto-ng invariant and fee model.
#[derive(Debug, Default)]
pub struct TricryptoStrategy;

#[async_trait]
impl<P: Provider + Send + Sync + 'static + ?Sized> SwapStrategy<P> for TricryptoStrategy {
    async fn calculate_dy(&self, params: &SwapParams<'_, P>) -> Result<U256, ArbRsError> {
        let SwapParams {
            i,
            j,
            dx,
            pool,
            block_timestamp,
            block_number,
        } = params;
        let (i, j, dx, block_timestamp, _block_number) =
            (*i, *j, *dx, *block_timestamp, *block_number);

        let attributes = &pool.attributes;
        let balances = pool.balances.read().await;

        let block_number = pool.provider.get_block_number().await?;

        let price_scale = pool.get_tricrypto_price_scale(block_number).await?;
        let gamma = pool.get_tricrypto_gamma(block_number).await?;
        let d = pool.get_tricrypto_d(block_number).await?;
        let amp = pool.a_precise(block_timestamp).await?;

        let precisions = [
            U256::from(10).pow(U256::from(12)),
            U256::from(10).pow(U256::from(10)),
            U256::from(1),
        ];

        let mut xp = balances.clone();
        xp[i] += dx;

        xp[0] *= precisions[0];
        for k in 0..(attributes.n_coins - 1) {
            xp[k + 1] = xp[k + 1]
                .checked_mul(price_scale[k])
                .ok_or(ArbRsError::CalculationError("xp mul overflow".to_string()))?
                .checked_mul(precisions[k + 1])
                .ok_or(ArbRsError::CalculationError("xp mul2 overflow".to_string()))?
                .checked_div(PRECISION)
                .ok_or(ArbRsError::CalculationError("xp div underflow".to_string()))?;
        }

        let y = tricrypto_math::newton_y(amp, gamma, &xp, d, j)?;
        let mut dy = xp[j].saturating_sub(y).saturating_sub(U256::from(1));

        if j > 0 {
            dy = dy
                .checked_mul(PRECISION)
                .ok_or(ArbRsError::CalculationError("dy mul overflow".to_string()))?
                .checked_div(price_scale[j - 1])
                .ok_or(ArbRsError::CalculationError("dy div underflow".to_string()))?;
        }
        dy /= precisions[j];

        let mut xp_post_swap = xp.clone();
        xp_post_swap[j] = y;
        let fee_gamma = attributes.fee_gamma.unwrap_or_default();
        let mid_fee = attributes.mid_fee.unwrap_or_default();
        let out_fee = attributes.out_fee.unwrap_or_default();

        let f = tricrypto_math::reduction_coefficient(&xp_post_swap, fee_gamma)?;
        let fee_calc = (mid_fee.checked_mul(f).ok_or(ArbRsError::CalculationError(
            "fee_calc mul overflow".to_string(),
        ))? + out_fee.checked_mul(TEN_POW_18 - f).ok_or(
            ArbRsError::CalculationError("fee_calc mul2 overflow".to_string()),
        )?)
        .checked_div(TEN_POW_18)
        .ok_or(ArbRsError::CalculationError(
            "fee_calc div underflow".to_string(),
        ))?;

        let fee_amount = dy
            .checked_mul(fee_calc)
            .ok_or(ArbRsError::CalculationError(
                "fee_amount mul overflow".to_string(),
            ))?
            .checked_div(U256::from(10).pow(U256::from(10)))
            .ok_or(ArbRsError::CalculationError(
                "fee_amount div underflow".to_string(),
            ))?;

        Ok(dy.saturating_sub(fee_amount))
    }

    async fn calculate_dx(
        &self,
        _params: &SwapParams<'_, P>,
        _dy: U256,
    ) -> Result<U256, ArbRsError> {
        unimplemented!("Inverse Tricrypto calculation is not yet implemented.")
    }
}

/// Strategy for oracle-based pools that also use net admin balances.
#[derive(Debug, Default)]
pub struct OracleStrategy;

#[async_trait]
impl<P: Provider + Send + Sync + 'static + ?Sized> SwapStrategy<P> for OracleStrategy {
    async fn calculate_dy(&self, params: &SwapParams<'_, P>) -> Result<U256, ArbRsError> {
        let SwapParams {
            i,
            j,
            dx,
            pool,
            block_timestamp,
            block_number,
        } = params;
        let (i, j, dx, block_timestamp, _block_number) =
            (*i, *j, *dx, *block_timestamp, *block_number);

        let attributes = &pool.attributes;
        let fee = *pool.fee.read().await;

        let net_balances = pool.balances.read().await.clone();

        let block_number = pool.provider.get_block_number().await?;
        let rates = pool.get_oracle_rates(block_number).await?;

        let amp = pool.a_precise(block_timestamp).await?;
        let xp = math::xp(&rates, &net_balances)?;

        let dx_scaled = dx
            .checked_mul(rates[i])
            .ok_or_else(|| ArbRsError::CalculationError("dx_scaled mul overflow".to_string()))?
            .checked_div(PRECISION)
            .ok_or_else(|| ArbRsError::CalculationError("dx_scaled div underflow".to_string()))?;

        let x = xp[i]
            .checked_add(dx_scaled)
            .ok_or_else(|| ArbRsError::CalculationError("x add overflow".to_string()))?;

        let is_y0 = Y_VARIANT_GROUP_0.contains(&pool.address);
        let is_y1 = Y_VARIANT_GROUP_1.contains(&pool.address);
        let y = math::get_y(
            i,
            j,
            x,
            &xp,
            amp,
            attributes.n_coins,
            attributes.d_variant,
            is_y0,
            is_y1,
        )?;

        let dy = xp[j].saturating_sub(y).saturating_sub(U256::from(1));

        let fee_amount = dy
            .checked_mul(fee)
            .ok_or_else(|| ArbRsError::CalculationError("fee_amount mul overflow".to_string()))?
            .checked_div(FEE_DENOMINATOR)
            .ok_or_else(|| ArbRsError::CalculationError("fee_amount div underflow".to_string()))?;

        let dy_after_fee = dy.saturating_sub(fee_amount);

        let rate_j = rates[j];
        if rate_j.is_zero() {
            return Err(ArbRsError::CalculationError(
                "Output token rate is zero".to_string(),
            ));
        }

        let final_dy = dy_after_fee
            .checked_mul(PRECISION)
            .ok_or_else(|| ArbRsError::CalculationError("final_dy mul overflow".to_string()))?
            .checked_div(rate_j)
            .ok_or_else(|| ArbRsError::CalculationError("final_dy div underflow".to_string()))?;

        Ok(final_dy)
    }

    async fn calculate_dx(&self, params: &SwapParams<'_, P>, dy: U256) -> Result<U256, ArbRsError> {
        let balances = params.pool.balances.read().await;
        let attributes = &params.pool.attributes;
        let fee = *params.pool.fee.read().await;

        let rates = params.pool.get_oracle_rates(params.block_number).await?;

        let amp = params.pool.a_precise(params.block_timestamp).await?;
        let xp = math::xp(&rates, &*balances)?;

        let dy_plus_fee = dy
            .checked_mul(FEE_DENOMINATOR)
            .ok_or_else(|| ArbRsError::CalculationError("dy_plus_fee mul overflow".to_string()))?
            .checked_div(FEE_DENOMINATOR.saturating_sub(fee))
            .ok_or_else(|| ArbRsError::CalculationError("dy_plus_fee div underflow".to_string()))?;

        let dy_scaled = dy_plus_fee
            .checked_mul(rates[params.j])
            .ok_or_else(|| ArbRsError::CalculationError("dy_scaled mul overflow".to_string()))?
            .checked_div(PRECISION)
            .ok_or_else(|| ArbRsError::CalculationError("dy_scaled div underflow".to_string()))?;

        let y = xp[params.j]
            .checked_sub(dy_scaled)
            .ok_or_else(|| ArbRsError::CalculationError("y sub underflow".to_string()))?;

        let is_y0 = Y_VARIANT_GROUP_0.contains(&params.pool.address);
        let is_y1 = Y_VARIANT_GROUP_1.contains(&params.pool.address);
        let x = math::get_y(
            params.j,
            params.i,
            y,
            &xp,
            amp,
            attributes.n_coins,
            attributes.d_variant,
            is_y0,
            is_y1,
        )?;

        let dx_scaled = x
            .checked_sub(xp[params.i])
            .ok_or_else(|| ArbRsError::CalculationError("dx_scaled sub underflow".to_string()))?;

        let rate_i = rates[params.i];
        if rate_i.is_zero() {
            return Err(ArbRsError::CalculationError(
                "Input token rate is zero".to_string(),
            ));
        }

        Ok(dx_scaled
            .checked_mul(PRECISION)
            .ok_or_else(|| ArbRsError::CalculationError("final_dx mul overflow".to_string()))?
            .checked_div(rate_i)
            .ok_or_else(|| ArbRsError::CalculationError("final_dx div underflow".to_string()))?
            .saturating_add(U256::from(1)))
    }
}

/// Strategy for pools that require subtracting admin fees from balances before calculation.
#[derive(Debug, Default)]
pub struct AdminFeeStrategy;

#[async_trait]
impl<P: Provider + Send + Sync + 'static + ?Sized> SwapStrategy<P> for AdminFeeStrategy {
    async fn calculate_dy(&self, params: &SwapParams<'_, P>) -> Result<U256, ArbRsError> {
        let SwapParams {
            i,
            j,
            dx,
            pool,
            block_timestamp,
            ..
        } = params;
        let (i, j, dx) = (*i, *j, *dx);

        let live_balances = pool.fetch_balances_by_balance_of().await?;
        let admin_balances = pool.get_admin_balances().await?;
        let net_balances: Vec<U256> = live_balances
            .iter()
            .zip(admin_balances.iter())
            .map(|(l, a)| l.saturating_sub(*a))
            .collect();
        println!("[AdminFeeStrategy] Live Balances:  {:?}", live_balances);
        println!("[AdminFeeStrategy] Admin Balances: {:?}", admin_balances);
        println!("[AdminFeeStrategy] Net Balances:   {:?}", net_balances);

        let attributes = &pool.attributes;
        let fee = *pool.fee.read().await;
        let amp = pool.a_precise(*block_timestamp).await?;
        let rates = &attributes.rates;
        let xp = math::xp(rates, &net_balances)?;

        let dx_scaled = dx
            .checked_mul(rates[i])
            .ok_or_else(|| ArbRsError::CalculationError("dx_scaled mul overflow".to_string()))?
            .checked_div(PRECISION)
            .ok_or_else(|| ArbRsError::CalculationError("dx_scaled div underflow".to_string()))?;

        let x = xp[i]
            .checked_add(dx_scaled)
            .ok_or_else(|| ArbRsError::CalculationError("x add overflow".to_string()))?;

        let is_y0 = Y_VARIANT_GROUP_0.contains(&pool.address);
        let is_y1 = Y_VARIANT_GROUP_1.contains(&pool.address);

        let y = math::get_y(
            i,
            j,
            x,
            &xp,
            amp,
            attributes.n_coins,
            DVariant::Legacy,
            is_y0,
            is_y1,
        )?;

        let dy = xp[j].saturating_sub(y).saturating_sub(U256::from(1));

        let fee_amount = dy
            .checked_mul(fee)
            .ok_or_else(|| ArbRsError::CalculationError("fee_amount mul overflow".to_string()))?
            .checked_div(FEE_DENOMINATOR)
            .ok_or_else(|| ArbRsError::CalculationError("fee_amount div underflow".to_string()))?;

        let dy_after_fee = dy.saturating_sub(fee_amount);

        let rate_j = rates[j];
        if rate_j.is_zero() {
            return Err(ArbRsError::CalculationError(
                "Output token rate is zero".to_string(),
            ));
        }

        let final_dy = dy_after_fee
            .checked_mul(PRECISION)
            .ok_or_else(|| ArbRsError::CalculationError("final_dy mul overflow".to_string()))?
            .checked_div(rate_j)
            .ok_or_else(|| ArbRsError::CalculationError("final_dy div underflow".to_string()))?;

        Ok(final_dy)
    }

    async fn calculate_dx(&self, params: &SwapParams<'_, P>, dy: U256) -> Result<U256, ArbRsError> {
        let SwapParams { i, j, pool, .. } = params;
        let (i, j) = (*i, *j);

        let balances = pool.balances.read().await;
        let attributes = &pool.attributes;
        let fee = *pool.fee.read().await;
        let amp = pool.a_precise(params.block_timestamp).await?;
        let rates = &attributes.rates;
        let xp = math::xp(rates, &*balances)?;

        let dy_plus_fee = dy
            .checked_mul(FEE_DENOMINATOR)
            .ok_or_else(|| ArbRsError::CalculationError("dy_plus_fee mul overflow".to_string()))?
            .checked_div(FEE_DENOMINATOR.saturating_sub(fee))
            .ok_or_else(|| ArbRsError::CalculationError("dy_plus_fee div underflow".to_string()))?;

        let dy_scaled = dy_plus_fee
            .checked_mul(rates[j])
            .ok_or_else(|| ArbRsError::CalculationError("dy_scaled mul overflow".to_string()))?
            .checked_div(PRECISION)
            .ok_or_else(|| ArbRsError::CalculationError("dy_scaled div underflow".to_string()))?;

        let y = xp[j]
            .checked_sub(dy_scaled)
            .ok_or_else(|| ArbRsError::CalculationError("y sub underflow".to_string()))?;

        let is_y0 = Y_VARIANT_GROUP_0.contains(&pool.address);
        let is_y1 = Y_VARIANT_GROUP_1.contains(&pool.address);
        let x = math::get_y(
            j,
            i,
            y,
            &xp,
            amp,
            attributes.n_coins,
            attributes.d_variant,
            is_y0,
            is_y1,
        )?;

        let dx_scaled = x
            .checked_sub(xp[i])
            .ok_or_else(|| ArbRsError::CalculationError("dx_scaled sub underflow".to_string()))?;

        let rate_i = rates[i];
        if rate_i.is_zero() {
            return Err(ArbRsError::CalculationError(
                "Input token rate is zero".to_string(),
            ));
        }

        Ok(dx_scaled
            .checked_mul(PRECISION)
            .ok_or_else(|| ArbRsError::CalculationError("final_dx mul overflow".to_string()))?
            .checked_div(rate_i)
            .ok_or_else(|| ArbRsError::CalculationError("final_dx div underflow".to_string()))?
            .saturating_add(U256::from(1)))
    }
}
