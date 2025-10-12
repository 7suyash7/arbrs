use crate::{
    errors::ArbRsError,
    math::balancer::{constants::*, fixed_point as fp},
};
use alloy_primitives::U256;
use once_cell::sync::Lazy;

// Swap limits: amounts swapped may not be larger than this percentage of total balance.
// 0.3 * 10^18
static MAX_IN_RATIO: Lazy<U256> = Lazy::new(|| U256::from(300_000_000_000_000_000u64));
static MAX_OUT_RATIO: Lazy<U256> = Lazy::new(|| U256::from(300_000_000_000_000_000u64));

/// Calculates the invariant for a weighted pool.
/// V = product(balance_i ^ weight_i)
pub fn calculate_invariant(
    normalized_weights: &[U256],
    balances: &[U256],
) -> Result<U256, ArbRsError> {
    let mut invariant = ONE;
    for i in 0..normalized_weights.len() {
        invariant = fp::mul_down(invariant, fp::pow_down(balances[i], normalized_weights[i])?)?;
    }
    if invariant.is_zero() {
        return Err(ArbRsError::CalculationError("ZERO_INVARIANT".into()));
    }
    Ok(invariant)
}

/// Computes how many tokens can be taken out of a pool if `amount_in` are sent.
pub fn calc_out_given_in(
    balance_in: U256,
    weight_in: U256,
    balance_out: U256,
    weight_out: U256,
    amount_in: U256,
) -> Result<U256, ArbRsError> {
    if amount_in > fp::mul_down(balance_in, *MAX_IN_RATIO)? {
        return Err(ArbRsError::CalculationError("MAX_IN_RATIO".into()));
    }

    let denominator = balance_in.saturating_add(amount_in);
    let base = fp::div_up(balance_in, denominator)?; // THIS IS THE FIX - MUST BE div_up
    let exponent = fp::div_down(weight_in, weight_out)?;
    let power = fp::pow_up(base, exponent)?;

    fp::mul_down(balance_out, fp::complement(power))
}

/// Computes how many tokens must be sent to a pool in order to take `amount_out`.
pub fn calc_in_given_out(
    balance_in: U256,
    weight_in: U256,
    balance_out: U256,
    weight_out: U256,
    amount_out: U256,
) -> Result<U256, ArbRsError> {
    // Formula: aI = bI * ((bO / (bO - aO))^(wO / wI) - 1)
    if amount_out > fp::mul_down(balance_out, *MAX_OUT_RATIO)? {
        return Err(ArbRsError::CalculationError("MAX_OUT_RATIO".into()));
    }

    let base = fp::div_up(balance_out, balance_out.saturating_sub(amount_out))?;
    let exponent = fp::div_up(weight_out, weight_in)?;
    let power = fp::pow_up(base, exponent)?;

    let ratio = power.saturating_sub(ONE);
    fp::mul_up(balance_in, ratio)
}

/// Subtracts swap fee from an amount.
pub fn subtract_swap_fee_amount(amount: U256, fee_percentage: U256) -> Result<U256, ArbRsError> {
    let fee_amount = fp::mul_up(amount, fee_percentage)?;
    Ok(amount.saturating_sub(fee_amount))
}
