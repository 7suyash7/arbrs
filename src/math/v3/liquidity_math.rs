use alloy_primitives::{U256, U512};

/// Adds a signed liquidity delta to an unsigned liquidity amount.
pub fn add_delta(liquidity: u128, delta: i128) -> Option<u128> {
    // FIX: Perform math directly on primitive types with checked operations.
    // This is cleaner and avoids the complex I256 conversion issues.
    if delta >= 0 {
        liquidity.checked_add(delta as u128)
    } else {
        // `unsigned_abs()` is the correct way to get the absolute value for subtraction.
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

    let numerator = amount0 << 96;
    let denominator = sqrt_ratio_b_x96 - sqrt_ratio_a_x96;

    if denominator.is_zero() {
        return None;
    }

    let ratio: U256 = numerator / denominator;

    if ratio > U256::from(u128::MAX) {
        None
    } else {
        Some(ratio.to::<u128>())
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

    let numerator1: U256 = amount1 << 96;
    let numerator2 = sqrt_ratio_b_x96 - sqrt_ratio_a_x96;

    let product = numerator1.widening_mul(numerator2);
    let denominator: U512 = sqrt_ratio_a_x96.widening_mul(sqrt_ratio_b_x96) >> 96;

    if denominator.is_zero() {
        return None;
    }

    let ratio: U512 = product / denominator;

    // FIX: Convert the U256 max value to a U512 for comparison.
    if ratio > U512::from(U256::from(u128::MAX)) {
        None
    } else {
        Some(ratio.to::<u128>())
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
