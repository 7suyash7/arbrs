use crate::curve::constants::{FEE_DENOMINATOR, PRECISION};
use crate::curve::pool::CurveStableswapPool;
use crate::curve::pool_overrides::{DVariant, Y_VARIANT_GROUP_0, Y_VARIANT_GROUP_1};
use crate::curve::tricrypto_math::TEN_POW_18;
use crate::curve::types::CurvePoolSnapshot;
use crate::curve::{math, tricrypto_math};
use crate::errors::ArbRsError;
use alloy_primitives::{Address, U256, address};
use alloy_provider::Provider;

const STETH_USDC_METAPOOL: Address = address!("C61557C5d177bd7DC889A3b621eEC333e168f68A");
const RETH_ETH_METAPOOL: Address = address!("618788357D0EBd8A37e763ADab3bc575D54c2C7d");
const COMPOUND_POOL_ADDRESS: Address = address!("A2B47E3D5c44877cca798226B7B8118F9BFb7A56");
const AAVE_POOL_ADDRESS: Address = address!("52EA46506B9CC5Ef470C5bf89f17Dc28bB35D85C");
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

/// A synchronous parameter struct that holds a snapshot of the pool state.
pub struct SwapParams<'a, P: Provider + Send + Sync + 'static + ?Sized> {
    pub i: usize,
    pub j: usize,
    pub dx: U256,
    pub pool: &'a CurveStableswapPool<P>,
    pub snapshot: &'a CurvePoolSnapshot,
}

/// The synchronous trait for all swap calculation strategies.
pub trait SwapStrategy<P: Provider + Send + Sync + 'static + ?Sized> {
    fn calculate_dy(&self, params: &SwapParams<P>) -> Result<U256, ArbRsError>;
    fn calculate_dx(&self, params: &SwapParams<P>, dy: U256) -> Result<U256, ArbRsError>;
}

/// Strategy for standard Curve V1 pools.
/// Logic: xp -> x -> y -> dy -> fee -> unscale by rate
#[derive(Debug, Default)]
pub struct DefaultStrategy;
impl<P: Provider + Send + Sync + 'static + ?Sized> SwapStrategy<P> for DefaultStrategy {
    fn calculate_dy(&self, params: &SwapParams<P>) -> Result<U256, ArbRsError> {
        let (i, j, dx) = (params.i, params.j, params.dx);
        let attributes = &params.pool.attributes;

        let balances = &params.snapshot.balances;
        let fee = params.snapshot.fee;
        let amp = params.snapshot.a;
        let rates = &params.snapshot.rates;

        let xp = math::xp(rates, balances)?;

        let dx_scaled = (dx * rates[i])
            .checked_div(PRECISION)
            .ok_or_else(|| ArbRsError::CalculationError("dx_scaled division failed".to_string()))?;

        let x = xp[i]
            .checked_add(dx_scaled)
            .ok_or_else(|| ArbRsError::CalculationError("x addition failed".to_string()))?;

        let is_y0 = Y_VARIANT_GROUP_0.contains(&params.pool.address);
        let is_y1 = Y_VARIANT_GROUP_1.contains(&params.pool.address);
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

        let fee_amount = (dy * fee).checked_div(FEE_DENOMINATOR).ok_or_else(|| {
            ArbRsError::CalculationError("fee_amount division failed".to_string())
        })?;

        let dy_after_fee = dy.saturating_sub(fee_amount);

        let rate_j = rates[j];
        if rate_j.is_zero() {
            return Err(ArbRsError::CalculationError("Rate is zero".into()));
        }

        (dy_after_fee * PRECISION)
            .checked_div(rate_j)
            .ok_or_else(|| ArbRsError::CalculationError("final dy division failed".to_string()))
    }

    fn calculate_dx(&self, params: &SwapParams<P>, dy: U256) -> Result<U256, ArbRsError> {
        let (i, j) = (params.i, params.j);
        let attributes = &params.pool.attributes;

        let balances = &params.snapshot.balances;
        let fee = params.snapshot.fee;
        let amp = params.snapshot.a;
        let rates = &params.snapshot.rates;

        let xp = math::xp(rates, balances)?;

        let dy_plus_fee = (dy * FEE_DENOMINATOR)
            .checked_div(FEE_DENOMINATOR.saturating_sub(fee))
            .ok_or_else(|| {
                ArbRsError::CalculationError("dy_plus_fee division failed".to_string())
            })?;

        let dy_scaled = (dy_plus_fee * rates[j])
            .checked_div(PRECISION)
            .ok_or_else(|| ArbRsError::CalculationError("dy_scaled division failed".to_string()))?;

        let y = xp[j]
            .checked_sub(dy_scaled)
            .ok_or_else(|| ArbRsError::CalculationError("y subtraction failed".to_string()))?;

        let is_y0 = Y_VARIANT_GROUP_0.contains(&params.pool.address);
        let is_y1 = Y_VARIANT_GROUP_1.contains(&params.pool.address);
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

        let dx_scaled = x.checked_sub(xp[i]).ok_or_else(|| {
            ArbRsError::CalculationError("dx_scaled subtraction failed".to_string())
        })?;

        let rate_i = rates[i];
        if rate_i.is_zero() {
            return Err(ArbRsError::CalculationError("Rate is zero".into()));
        }

        let final_dx = (dx_scaled * PRECISION)
            .checked_div(rate_i)
            .ok_or_else(|| ArbRsError::CalculationError("final_dx division failed".to_string()))?;

        Ok(final_dx.saturating_add(U256::from(1)))
    }
}

#[derive(Debug, Default)]
pub struct MetapoolStrategy;
impl<P: Provider + Send + Sync + 'static + ?Sized> SwapStrategy<P> for MetapoolStrategy {
    fn calculate_dy(&self, params: &SwapParams<P>) -> Result<U256, ArbRsError> {
        let (i, j, dx) = (params.i, params.j, params.dx);
        let attributes = &params.pool.attributes;

        let balances = &params.snapshot.balances;
        let fee = params.snapshot.fee;
        let amp = params.snapshot.a;
        let virtual_price = params.snapshot.base_pool_virtual_price.ok_or_else(|| {
            ArbRsError::CalculationError("Metapool virtual price not in snapshot".to_string())
        })?;

        let rates = match params.pool.address {
            STETH_USDC_METAPOOL => vec![PRECISION, virtual_price],
            RETH_ETH_METAPOOL => vec![
                params.snapshot.scaled_redemption_price.ok_or_else(|| {
                    ArbRsError::CalculationError("Missing scaled redemption price".to_string())
                })?,
                virtual_price,
            ],
            _ => vec![attributes.rates[0], virtual_price],
        };

        let xp = math::xp(&rates, balances)?;
        let dx_scaled = (dx * rates[i])
            .checked_div(PRECISION)
            .ok_or_else(|| ArbRsError::CalculationError("Metapool dy: dx_scaled failed".into()))?;
        let x = xp[i]
            .checked_add(dx_scaled)
            .ok_or_else(|| ArbRsError::CalculationError("Metapool dy: x addition failed".into()))?;

        let is_y0 = Y_VARIANT_GROUP_0.contains(&params.pool.address);
        let is_y1 = Y_VARIANT_GROUP_1.contains(&params.pool.address);
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
        let fee_amount = (dy * fee)
            .checked_div(FEE_DENOMINATOR)
            .ok_or_else(|| ArbRsError::CalculationError("Metapool dy: fee_amount failed".into()))?;
        let dy_after_fee = dy.saturating_sub(fee_amount);

        let rate_j = rates[j];
        if rate_j.is_zero() {
            return Err(ArbRsError::CalculationError("Rate is zero".into()));
        }

        (dy_after_fee * PRECISION)
            .checked_div(rate_j)
            .ok_or_else(|| {
                ArbRsError::CalculationError("Metapool dy: final division failed".into())
            })
    }

    fn calculate_dx(&self, params: &SwapParams<P>, dy: U256) -> Result<U256, ArbRsError> {
        let (i, j) = (params.i, params.j);
        let attributes = &params.pool.attributes;

        let balances = &params.snapshot.balances;
        let fee = params.snapshot.fee;
        let amp = params.snapshot.a;
        let virtual_price = params.snapshot.base_pool_virtual_price.ok_or_else(|| {
            ArbRsError::CalculationError("Metapool virtual price not in snapshot".to_string())
        })?;

        let rates = match params.pool.address {
            STETH_USDC_METAPOOL => vec![PRECISION, virtual_price],
            RETH_ETH_METAPOOL => vec![
                params.snapshot.scaled_redemption_price.ok_or_else(|| {
                    ArbRsError::CalculationError("Missing scaled redemption price".to_string())
                })?,
                virtual_price,
            ],
            _ => vec![attributes.rates[0], virtual_price],
        };

        let xp = math::xp(&rates, balances)?;

        let dy_plus_fee = (dy * FEE_DENOMINATOR)
            .checked_div(FEE_DENOMINATOR.saturating_sub(fee))
            .ok_or_else(|| {
                ArbRsError::CalculationError("Metapool dx: dy_plus_fee failed".into())
            })?;
        let dy_scaled = (dy_plus_fee * rates[j])
            .checked_div(PRECISION)
            .ok_or_else(|| ArbRsError::CalculationError("Metapool dx: dy_scaled failed".into()))?;
        let y = xp[j].checked_sub(dy_scaled).ok_or_else(|| {
            ArbRsError::CalculationError("Metapool dx: y subtraction failed".into())
        })?;

        let is_y0 = Y_VARIANT_GROUP_0.contains(&params.pool.address);
        let is_y1 = Y_VARIANT_GROUP_1.contains(&params.pool.address);
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

        let dx_scaled = x.checked_sub(xp[i]).ok_or_else(|| {
            ArbRsError::CalculationError("Metapool dx: dx_scaled subtraction failed".into())
        })?;
        let rate_i = rates[i];
        if rate_i.is_zero() {
            return Err(ArbRsError::CalculationError("Rate is zero".into()));
        }

        let final_dx = (dx_scaled * PRECISION).checked_div(rate_i).ok_or_else(|| {
            ArbRsError::CalculationError("Metapool dx: final division failed".into())
        })?;
        Ok(final_dx.saturating_add(U256::from(1)))
    }
}

#[derive(Debug, Default)]
pub struct LendingStrategy;
impl<P: Provider + Send + Sync + 'static + ?Sized> SwapStrategy<P> for LendingStrategy {
    fn calculate_dy(&self, params: &SwapParams<P>) -> Result<U256, ArbRsError> {
        let (i, j, dx) = (params.i, params.j, params.dx);

        let balances = &params.snapshot.balances;
        let fee = params.snapshot.fee;
        let amp = params.snapshot.a;
        let rates = &params.snapshot.rates;

        let xp = math::xp(rates, balances)?;
        let dx_scaled = (dx * rates[i])
            .checked_div(PRECISION)
            .ok_or_else(|| ArbRsError::CalculationError("Lending dy: dx_scaled failed".into()))?;
        let x = xp[i]
            .checked_add(dx_scaled)
            .ok_or_else(|| ArbRsError::CalculationError("Lending dy: x addition failed".into()))?;

        let is_y0 = Y_VARIANT_GROUP_0.contains(&params.pool.address);
        let is_y1 = Y_VARIANT_GROUP_1.contains(&params.pool.address);
        let y = math::get_y(
            i,
            j,
            x,
            &xp,
            amp,
            params.pool.attributes.n_coins,
            params.pool.attributes.d_variant,
            is_y0,
            is_y1,
        )?;

        let dy_raw = xp[j].saturating_sub(y);

        if LENDING_GROUP_A.contains(&params.pool.address) {
            let fee_amount = (dy_raw * fee).checked_div(FEE_DENOMINATOR).ok_or_else(|| {
                ArbRsError::CalculationError("Lending dy: fee_amount A failed".into())
            })?;
            let dy_after_fee = dy_raw.saturating_sub(fee_amount);
            if rates[j].is_zero() {
                return Err(ArbRsError::CalculationError("Rate is zero".into()));
            }
            (dy_after_fee * PRECISION)
                .checked_div(rates[j])
                .ok_or_else(|| ArbRsError::CalculationError("Lending dy: final dy A failed".into()))
        } else if LENDING_GROUP_B.contains(&params.pool.address) {
            let fee_amount = (dy_raw * fee).checked_div(FEE_DENOMINATOR).ok_or_else(|| {
                ArbRsError::CalculationError("Lending dy: fee_amount B failed".into())
            })?;
            Ok(dy_raw.saturating_sub(fee_amount))
        } else {
            let dy_with_margin = dy_raw.saturating_sub(U256::from(1));
            if rates[j].is_zero() {
                return Err(ArbRsError::CalculationError("Rate is zero".into()));
            }
            let final_dy = (dy_with_margin * PRECISION)
                .checked_div(rates[j])
                .ok_or_else(|| {
                    ArbRsError::CalculationError("Lending dy: final_dy else failed".into())
                })?;
            let fee_amount = (final_dy * fee)
                .checked_div(FEE_DENOMINATOR)
                .ok_or_else(|| {
                    ArbRsError::CalculationError("Lending dy: fee_amount else failed".into())
                })?;
            Ok(final_dy.saturating_sub(fee_amount))
        }
    }

    fn calculate_dx(&self, params: &SwapParams<P>, dy: U256) -> Result<U256, ArbRsError> {
        let (i, j) = (params.i, params.j);

        let balances = &params.snapshot.balances;
        let fee = params.snapshot.fee;
        let amp = params.snapshot.a;
        let rates = &params.snapshot.rates;

        let xp = math::xp(rates, balances)?;

        let dy_plus_fee = (dy * FEE_DENOMINATOR)
            .checked_div(FEE_DENOMINATOR.saturating_sub(fee))
            .ok_or_else(|| ArbRsError::CalculationError("Lending dx: dy_plus_fee failed".into()))?;
        let dy_scaled = (dy_plus_fee * rates[j])
            .checked_div(PRECISION)
            .ok_or_else(|| ArbRsError::CalculationError("Lending dx: dy_scaled failed".into()))?;
        let y = xp[j].checked_sub(dy_scaled).ok_or_else(|| {
            ArbRsError::CalculationError("Lending dx: y subtraction failed".into())
        })?;

        let is_y0 = Y_VARIANT_GROUP_0.contains(&params.pool.address);
        let is_y1 = Y_VARIANT_GROUP_1.contains(&params.pool.address);
        let x = math::get_y(
            j,
            i,
            y,
            &xp,
            amp,
            params.pool.attributes.n_coins,
            params.pool.attributes.d_variant,
            is_y0,
            is_y1,
        )?;

        let dx_scaled = x.checked_sub(xp[i]).ok_or_else(|| {
            ArbRsError::CalculationError("Lending dx: dx_scaled subtraction failed".into())
        })?;
        let rate_i = rates[i];
        if rate_i.is_zero() {
            return Err(ArbRsError::CalculationError("Rate is zero".into()));
        }

        let final_dx = (dx_scaled * PRECISION).checked_div(rate_i).ok_or_else(|| {
            ArbRsError::CalculationError("Lending dx: final_dx division failed".into())
        })?;
        Ok(final_dx.saturating_add(U256::from(1)))
    }
}

#[derive(Debug, Default)]
pub struct UnscaledStrategy;
impl<P: Provider + Send + Sync + 'static + ?Sized> SwapStrategy<P> for UnscaledStrategy {
    fn calculate_dy(&self, params: &SwapParams<P>) -> Result<U256, ArbRsError> {
        let (i, j, dx) = (params.i, params.j, params.dx);
        let attributes = &params.pool.attributes;

        let balances = &params.snapshot.balances;
        let fee = params.snapshot.fee;
        let amp = params.snapshot.a;

        let xp = balances.clone();

        let x = xp[i]
            .checked_add(dx)
            .ok_or_else(|| ArbRsError::CalculationError("x add overflow".to_string()))?;

        let is_y0 = Y_VARIANT_GROUP_0.contains(&params.pool.address);
        let is_y1 = Y_VARIANT_GROUP_1.contains(&params.pool.address);
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

        let fee_amount = (dy * fee).checked_div(FEE_DENOMINATOR).ok_or_else(|| {
            ArbRsError::CalculationError("fee_amount division failed".to_string())
        })?;

        let final_dy = dy.saturating_sub(fee_amount);

        Ok(final_dy)
    }

    fn calculate_dx(&self, params: &SwapParams<P>, dy: U256) -> Result<U256, ArbRsError> {
        let balances = &params.snapshot.balances;
        let fee = params.snapshot.fee;
        let amp = params.snapshot.a;

        let xp = balances.clone();

        let dy_plus_fee = (dy * FEE_DENOMINATOR)
            .checked_div(FEE_DENOMINATOR.saturating_sub(fee))
            .ok_or_else(|| {
                ArbRsError::CalculationError("dy_plus_fee division failed".to_string())
            })?;

        let y = xp[params.j]
            .checked_sub(dy_plus_fee)
            .ok_or_else(|| ArbRsError::CalculationError("y subtraction failed".to_string()))?;

        let is_y0 = Y_VARIANT_GROUP_0.contains(&params.pool.address);
        let is_y1 = Y_VARIANT_GROUP_1.contains(&params.pool.address);
        let x = math::get_y(
            params.j,
            params.i,
            y,
            &xp,
            amp,
            params.pool.attributes.n_coins,
            params.pool.attributes.d_variant,
            is_y0,
            is_y1,
        )?;

        Ok(x.checked_sub(xp[params.i])
            .ok_or_else(|| ArbRsError::CalculationError("dx subtraction failed".to_string()))?
            .saturating_add(U256::from(1)))
    }
}

// Quick Note on Dynamic Fee Logic
// Your original implementation for this strategy followed the same calculation path as DefaultStrategy. A true dynamic fee calculation (like for stETH) would use the offpeg_fee_multiplier from PoolAttributes and the dynamic_fee function from your curve/math.rs file to adjust the fee based on how far the pool is from its peg.

// The code I provided above faithfully refactors your current logic. After we finish this big refactor, we can easily circle back and enhance this strategy to implement the true dynamic fee math.
#[derive(Debug, Default)]
pub struct DynamicFeeStrategy;
impl<P: Provider + Send + Sync + 'static + ?Sized> SwapStrategy<P> for DynamicFeeStrategy {
    fn calculate_dy(&self, params: &SwapParams<P>) -> Result<U256, ArbRsError> {
        DefaultStrategy::default().calculate_dy(params)
    }

    fn calculate_dx(&self, params: &SwapParams<P>, dy: U256) -> Result<U256, ArbRsError> {
        DefaultStrategy::default().calculate_dx(params, dy)
    }
}

#[derive(Debug, Default)]
pub struct TricryptoStrategy;
impl<P: Provider + Send + Sync + 'static + ?Sized> SwapStrategy<P> for TricryptoStrategy {
    fn calculate_dy(&self, params: &SwapParams<P>) -> Result<U256, ArbRsError> {
        let (i, j, dx) = (params.i, params.j, params.dx);
        let attributes = &params.pool.attributes;
        let snapshot = params.snapshot;

        let balances = &snapshot.balances;
        let amp = snapshot.a;
        let price_scale = snapshot.tricrypto_price_scale.as_ref().ok_or_else(|| {
            ArbRsError::CalculationError("Missing tricrypto price_scale in snapshot".to_string())
        })?;
        let gamma = snapshot.tricrypto_gamma.ok_or_else(|| {
            ArbRsError::CalculationError("Missing tricrypto gamma in snapshot".to_string())
        })?;
        let d = snapshot.tricrypto_d.ok_or_else(|| {
            ArbRsError::CalculationError("Missing tricrypto D in snapshot".to_string())
        })?;

        let precisions = [
            U256::from(10).pow(U256::from(12)),
            U256::from(10).pow(U256::from(10)),
            U256::from(1),
        ];

        let mut xp = balances.clone();
        xp[i] += dx;

        xp[0] *= precisions[0];
        for k in 0..(attributes.n_coins - 1) {
            xp[k + 1] = (xp[k + 1] * price_scale[k] * precisions[k + 1])
                .checked_div(PRECISION)
                .ok_or_else(|| ArbRsError::CalculationError("xp div underflow".to_string()))?;
        }

        let y = tricrypto_math::newton_y(amp, gamma, &xp, d, j)?;
        let mut dy = xp[j].saturating_sub(y).saturating_sub(U256::from(1));

        if j > 0 {
            dy = (dy * PRECISION)
                .checked_div(price_scale[j - 1])
                .ok_or_else(|| ArbRsError::CalculationError("dy div underflow".to_string()))?;
        }
        dy /= precisions[j];

        let mut xp_post_swap = xp;
        xp_post_swap[j] = y;
        let fee_gamma = attributes.fee_gamma.unwrap_or_default();
        let mid_fee = attributes.mid_fee.unwrap_or_default();
        let out_fee = attributes.out_fee.unwrap_or_default();

        let f = tricrypto_math::reduction_coefficient(&xp_post_swap, fee_gamma)?;
        let fee_calc = (mid_fee * f + out_fee * (TEN_POW_18 - f))
            .checked_div(TEN_POW_18)
            .ok_or_else(|| ArbRsError::CalculationError("fee_calc div underflow".to_string()))?;

        let fee_amount = (dy * fee_calc)
            .checked_div(U256::from(10).pow(U256::from(10)))
            .ok_or_else(|| ArbRsError::CalculationError("fee_amount div underflow".to_string()))?;

        Ok(dy.saturating_sub(fee_amount))
    }

    fn calculate_dx(&self, _params: &SwapParams<P>, _dy: U256) -> Result<U256, ArbRsError> {
        unimplemented!("Inverse Tricrypto calculation is not yet implemented.")
    }
}

#[derive(Debug, Default)]
pub struct OracleStrategy;
impl<P: Provider + Send + Sync + 'static + ?Sized> SwapStrategy<P> for OracleStrategy {
    fn calculate_dy(&self, params: &SwapParams<P>) -> Result<U256, ArbRsError> {
        DefaultStrategy::default().calculate_dy(params)
    }

    fn calculate_dx(&self, params: &SwapParams<P>, dy: U256) -> Result<U256, ArbRsError> {
        DefaultStrategy::default().calculate_dx(params, dy)
    }
}

#[derive(Debug, Default)]
pub struct AdminFeeStrategy;
impl<P: Provider + Send + Sync + 'static + ?Sized> SwapStrategy<P> for AdminFeeStrategy {
    fn calculate_dy(&self, params: &SwapParams<P>) -> Result<U256, ArbRsError> {
        let (i, j, dx) = (params.i, params.j, params.dx);
        let attributes = &params.pool.attributes;

        let net_balances = &params.snapshot.balances;
        let fee = params.snapshot.fee;
        let amp = params.snapshot.a;
        let rates = &params.snapshot.rates;

        let xp = math::xp(rates, net_balances)?;
        let dx_scaled = (dx * rates[i])
            .checked_div(PRECISION)
            .ok_or_else(|| ArbRsError::CalculationError("dx_scaled failed".into()))?;
        let x = xp[i]
            .checked_add(dx_scaled)
            .ok_or_else(|| ArbRsError::CalculationError("x addition failed".into()))?;

        let is_y0 = Y_VARIANT_GROUP_0.contains(&params.pool.address);
        let is_y1 = Y_VARIANT_GROUP_1.contains(&params.pool.address);

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
        let fee_amount = (dy * fee)
            .checked_div(FEE_DENOMINATOR)
            .ok_or_else(|| ArbRsError::CalculationError("fee_amount division failed".into()))?;
        let dy_after_fee = dy.saturating_sub(fee_amount);

        let rate_j = rates[j];
        if rate_j.is_zero() {
            return Err(ArbRsError::CalculationError("Rate is zero".into()));
        }

        (dy_after_fee * PRECISION)
            .checked_div(rate_j)
            .ok_or_else(|| ArbRsError::CalculationError("final dy division failed".into()))
    }

    fn calculate_dx(&self, params: &SwapParams<P>, dy: U256) -> Result<U256, ArbRsError> {
        DefaultStrategy::default().calculate_dx(params, dy)
    }
}
