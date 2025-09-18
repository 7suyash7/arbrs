use crate::math::v3::{constants::Q96, full_math};
use alloy_primitives::U256;

/// Adds a signed liquidity delta to an unsigned liquidity amount.
pub fn add_delta(liquidity: u128, delta: i128) -> Option<u128> {
    if delta >= 0 {
        liquidity.checked_add(delta as u128)
    } else {
        liquidity.checked_sub(delta.unsigned_abs())
    }
}

/// Computes the amount of liquidity received for a given amount of token0 and a price range.
pub fn get_liquidity_for_amount0(
    sqrt_ratio_a_x96: U256,
    sqrt_ratio_b_x96: U256,
    amount0: U256,
) -> Option<u128> {
    let (mut sqrt_ratio_a_x96, mut sqrt_ratio_b_x96) = (sqrt_ratio_a_x96, sqrt_ratio_b_x96);
    if sqrt_ratio_a_x96 > sqrt_ratio_b_x96 {
        std::mem::swap(&mut sqrt_ratio_a_x96, &mut sqrt_ratio_b_x96);
    }

    let intermediate = full_math::mul_div(sqrt_ratio_a_x96, sqrt_ratio_b_x96, Q96)?;
    let numerator = amount0.checked_mul(intermediate)?;
    let denominator = sqrt_ratio_b_x96.checked_sub(sqrt_ratio_a_x96)?;

    if denominator.is_zero() {
        return None;
    }

    let liquidity = numerator.checked_div(denominator)?;

    if liquidity > U256::from(u128::MAX) {
        None
    } else {
        Some(liquidity.to::<u128>())
    }
}

/// Computes the amount of liquidity received for a given amount of token1 and a price range.
pub fn get_liquidity_for_amount1(
    sqrt_ratio_a_x96: U256,
    sqrt_ratio_b_x96: U256,
    amount1: U256,
) -> Option<u128> {
    let (mut sqrt_ratio_a_x96, mut sqrt_ratio_b_x96) = (sqrt_ratio_a_x96, sqrt_ratio_b_x96);
    if sqrt_ratio_a_x96 > sqrt_ratio_b_x96 {
        std::mem::swap(&mut sqrt_ratio_a_x96, &mut sqrt_ratio_b_x96);
    }

    let numerator = amount1.checked_mul(Q96)?;
    let denominator = sqrt_ratio_b_x96.checked_sub(sqrt_ratio_a_x96)?;

    if denominator.is_zero() {
        return None;
    }

    let liquidity = numerator.checked_div(denominator)?;

    if liquidity > U256::from(u128::MAX) {
        None
    } else {
        Some(liquidity.to::<u128>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_delta() {
        assert_eq!(add_delta(1, 0), Some(1));
        assert_eq!(add_delta(1, -1), Some(0));
        assert_eq!(add_delta(1, 1), Some(2));
        assert_eq!(add_delta(0, -1), None);
        assert_eq!(add_delta(3, -4), None);
        assert_eq!(add_delta(u128::MAX - 14, 15), None);
        assert_eq!(add_delta(u128::MAX, 1), None);
    }
}
