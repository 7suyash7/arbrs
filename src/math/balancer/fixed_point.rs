use crate::{
    errors::ArbRsError,
    math::balancer::{constants::*, log_exp_math},
};
use alloy_primitives::{U256, U512};
use num_bigint::BigInt;
use num_traits::Signed;

fn to_bigint(value: U256) -> BigInt {
    BigInt::from_bytes_be(num_bigint::Sign::Plus, &value.to_be_bytes::<32>())
}

pub fn to_u256(value: BigInt) -> Result<U256, ArbRsError> {
    if value.is_negative() || value.bits() > 256 {
        return Err(ArbRsError::CalculationError("BigInt to U256 conversion overflow".into()));
    }
    let (_, bytes) = value.to_bytes_be();
    let mut padded_bytes = [0u8; 32];
    padded_bytes[32 - bytes.len()..].copy_from_slice(&bytes);
    Ok(U256::from_be_bytes(padded_bytes))
}

pub fn mul_down(a: U256, b: U256) -> Result<U256, ArbRsError> {
    let product = a.widening_mul(b);
    let result = product / U512::from(ONE);
    if result > U512::from(U256::MAX) { Err(ArbRsError::CalculationError("Overflow converting U512 to U256".into())) } else { Ok(result.to()) }
}

pub fn mul_up(a: U256, b: U256) -> Result<U256, ArbRsError> {
    let product = a.widening_mul(b);
    if product.is_zero() { return Ok(U256::ZERO); }
    let result = (product - U512::from(1)) / U512::from(ONE) + U512::from(1);
    if result > U512::from(U256::MAX) { Err(ArbRsError::CalculationError("Overflow converting U512 to U256".into())) } else { Ok(result.to()) }
}

pub fn div_down(a: U256, b: U256) -> Result<U256, ArbRsError> {
    if b.is_zero() { return Err(ArbRsError::CalculationError("div_down by zero".into())); }
    if a.is_zero() { return Ok(U256::ZERO); }
    let a_inflated = a.widening_mul(ONE);
    let result = a_inflated / U512::from(b);
    if result > U512::from(U256::MAX) { Err(ArbRsError::CalculationError("Overflow converting U512 to U256".into())) } else { Ok(result.to()) }
}

pub fn div_up(a: U256, b: U256) -> Result<U256, ArbRsError> {
    if b.is_zero() { return Err(ArbRsError::CalculationError("div_up by zero".into())); }
    if a.is_zero() { return Ok(U256::ZERO); }
    let a_inflated = a.widening_mul(ONE);
    let result = (a_inflated - U512::from(1)) / U512::from(b) + U512::from(1);
    if result > U512::from(U256::MAX) { Err(ArbRsError::CalculationError("Overflow converting U512 to U256".into())) } else { Ok(result.to()) }
}

pub fn complement(x: U256) -> U256 {
    if x < ONE { ONE - x } else { U256::ZERO }
}

pub fn pow_down(x: U256, y: U256) -> Result<U256, ArbRsError> {
    if y == ONE { return Ok(x); }
    if y == TWO { return mul_down(x, x); }
    if y == FOUR {
        let square = mul_down(x, x)?;
        return mul_down(square, square);
    }
    let raw = log_exp_math::pow(&to_bigint(x), &to_bigint(y))?;
    let raw_u256 = to_u256(raw)?;
    let max_error = mul_up(raw_u256, MAX_POW_RELATIVE_ERROR)?.saturating_add(U256::from(1));
    Ok(raw_u256.saturating_sub(max_error))
}

pub fn pow_up(x: U256, y: U256) -> Result<U256, ArbRsError> {
    if y == ONE { return Ok(x); }
    if y == TWO { return mul_up(x, x); }
    if y == FOUR {
        let square = mul_up(x, x)?;
        return mul_up(square, square);
    }
    let raw = log_exp_math::pow(&to_bigint(x), &to_bigint(y))?;
    let raw_u256 = to_u256(raw)?;
    let max_error = mul_up(raw_u256, MAX_POW_RELATIVE_ERROR)?.saturating_add(U256::from(1));
    Ok(raw_u256.saturating_add(max_error))
}