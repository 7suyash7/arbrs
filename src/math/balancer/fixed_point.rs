use crate::{
    errors::ArbRsError,
    math::balancer::{constants::*, log_exp_math},
};
use alloy_primitives::{U256, U512};

/// Converts a U512 to a U256, returning an error on overflow.
fn to_u256(value: U512) -> Result<U256, ArbRsError> {
    if value > U512::from(U256::MAX) {
        Err(ArbRsError::CalculationError("Overflow converting U512 to U256".into()))
    } else {
        Ok(value.to())
    }
}

/// Equivalent to `floor(a * b)` in fixed-point math
pub fn mul_down(a: U256, b: U256) -> Result<U256, ArbRsError> {
    let product = a.widening_mul(b);
    let result = product / U512::from(ONE);
    to_u256(result)
}

/// Equivalent to `ceil(a * b)` in fixed-point math
pub fn mul_up(a: U256, b: U256) -> Result<U256, ArbRsError> {
    let product = a.widening_mul(b);
    if product.is_zero() { return Ok(U256::ZERO); }
    let result = (product - U512::from(1)) / U512::from(ONE) + U512::from(1);
    to_u256(result)
}

/// Equivalent to `floor(a / b)` in fixed-point math
pub fn div_down(a: U256, b: U256) -> Result<U256, ArbRsError> {
    if b.is_zero() { return Err(ArbRsError::CalculationError("div_down by zero".into())); }
    if a.is_zero() { return Ok(U256::ZERO); }
    let a_inflated = a.widening_mul(ONE);
    let result = a_inflated / U512::from(b);
    to_u256(result)
}

/// Equivalent to `ceil(a / b)` in fixed-point math
pub fn div_up(a: U256, b: U256) -> Result<U256, ArbRsError> {
    if b.is_zero() { return Err(ArbRsError::CalculationError("div_up by zero".into())); }
    if a.is_zero() { return Ok(U256::ZERO); }
    let a_inflated = a.widening_mul(ONE);
    let result = (a_inflated - U512::from(1)) / U512::from(b) + U512::from(1);
    to_u256(result)
}

/// Returns `1 - x` in fixed-point math
pub fn complement(x: U256) -> U256 {
    if x < ONE { ONE - x } else { U256::ZERO }
}

/// Calculates `x^y` rounding down.
pub fn pow_down(x: U256, y: U256) -> Result<U256, ArbRsError> {
    // Optimizations for common exponents
    if y == ONE { return Ok(x); }
    if y == TWO { return mul_down(x, x); }
    if y == FOUR {
        let square = mul_down(x, x)?;
        return mul_down(square, square);
    }

    let raw = log_exp_math::pow(x, y)?;
    let max_error = mul_up(raw, MAX_POW_RELATIVE_ERROR)?.saturating_add(U256::from(1));

    Ok(raw.saturating_sub(max_error))
}

/// Calculates `x^y` rounding up.
pub fn pow_up(x: U256, y: U256) -> Result<U256, ArbRsError> {
    // Optimizations for common exponents
    if y == ONE { return Ok(x); }
    if y == TWO { return mul_up(x, x); }
    if y == FOUR {
        let square = mul_up(x, x)?;
        return mul_up(square, square);
    }

    let raw = log_exp_math::pow(x, y)?;
    let max_error = mul_up(raw, MAX_POW_RELATIVE_ERROR)?.saturating_add(U256::from(1));

    Ok(raw.saturating_add(max_error))
}