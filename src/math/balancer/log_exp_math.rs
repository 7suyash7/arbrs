use crate::errors::ArbRsError;
use crate::math::balancer::constants::*;
use alloy_primitives::{U256, U512};
use num_bigint::BigInt;
use num_traits::Signed;
// Import the NEW official library
use balancer_maths_rust::common::maths::{pow_down_fixed, pow_up_fixed};

pub fn to_bigint(value: U256) -> BigInt {
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
    if result > U512::from(U256::MAX) { Err(ArbRsError::CalculationError("Overflow".into())) } else { Ok(result.to()) }
}

pub fn mul_up(a: U256, b: U256) -> Result<U256, ArbRsError> {
    let product = a.widening_mul(b);
    if product.is_zero() { return Ok(U256::ZERO); }
    let result = (product - U512::from(1)) / U512::from(ONE) + U512::from(1);
    if result > U512::from(U256::MAX) { Err(ArbRsError::CalculationError("Overflow".into())) } else { Ok(result.to()) }
}

pub fn div_down(a: U256, b: U256) -> Result<U256, ArbRsError> {
    if b.is_zero() { return Err(ArbRsError::CalculationError("div_down by zero".into())); }
    if a.is_zero() { return Ok(U256::ZERO); }
    let a_inflated = a.widening_mul(ONE);
    let result = a_inflated / U512::from(b);
    if result > U512::from(U256::MAX) { Err(ArbRsError::CalculationError("Overflow".into())) } else { Ok(result.to()) }
}

pub fn div_up(a: U256, b: U256) -> Result<U256, ArbRsError> {
    if b.is_zero() { return Err(ArbRsError::CalculationError("div_up by zero".into())); }
    if a.is_zero() { return Ok(U256::ZERO); }
    let a_inflated = a.widening_mul(ONE);
    let result = (a_inflated - U512::from(1)) / U512::from(b) + U512::from(1);
    if result > U512::from(U256::MAX) { Err(ArbRsError::CalculationError("Overflow".into())) } else { Ok(result.to()) }
}

pub fn complement(x: U256) -> U256 {
    if x < ONE { ONE - x } else { U256::ZERO }
}

// These functions now act as simple wrappers that call the official library
pub fn pow_down(x: U256, y: U256) -> Result<U256, ArbRsError> {
    let result_bigint = pow_down_fixed(&to_bigint(x), &to_bigint(y))?;
    to_u256(result_bigint)
}

pub fn pow_up(x: U256, y: U256) -> Result<U256, ArbRsError> {
    let result_bigint = pow_up_fixed(&to_bigint(x), &to_bigint(y))?;
    to_u256(result_bigint)
}