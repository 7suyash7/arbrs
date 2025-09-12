use alloy_primitives::{U256, U512};

/// Performs a multiplication and division in 512-bit precision.
/// Equivalent to (a * b) / denominator.
/// Reverts on division by zero or overflow.
///
/// # Arguments
/// * `a`: First operand.
/// * `b`: Second operand.
/// * `denominator`: Denominator.
///
/// # Returns
/// The result of (a * b) / denominator.
pub fn mul_div(a: U256, b: U256, denominator: U256) -> Option<U256> {
    if denominator.is_zero() {
        return None;
    }

    let product = a.widening_mul(b);
    let result = product / U512::from(denominator);

    if result > U512::from(U256::MAX) {
        None
    } else {
        Some(result.to())
    }
}

/// Performs a multiplication and division, rounding up.
/// Equivalent to ceil((a * b) / denominator).
/// Reverts on division by zero or overflow.
///
/// # Arguments
/// * `a`: First operand.
/// * `b`: Second operand.
/// * `denominator`: Denominator.
///
/// # Returns
/// The result of ceil((a * b) / denominator).
pub fn mul_div_rounding_up(a: U256, b: U256, denominator: U256) -> Option<U256> {
    if denominator.is_zero() {
        return None;
    }

    let product = a.widening_mul(b);
    let result = product / U512::from(denominator);

    if result >= U512::from(U256::MAX) {
        if result > U512::from(U256::MAX) { return None; }
        if product % U512::from(denominator) > U512::ZERO {
            return None;
        }
    }
    
    if product % U512::from(denominator) > U512::ZERO {
        Some(result.to::<U256>() + U256::from(1))
    } else {
        Some(result.to())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::U256;

    const Q128: U256 = U256::from_limbs([0, 0, 1, 0]);

    #[test]
    fn test_mul_div_reverts() {
        // Reverts on division by zero
        assert_eq!(mul_div(Q128, U256::from(5), U256::ZERO), None);
        assert_eq!(mul_div(Q128, Q128, U256::ZERO), None);
        
        // Reverts on overflow
        assert_eq!(mul_div(Q128, Q128, U256::from(1)), None);
        assert_eq!(mul_div(U256::MAX, U256::MAX, U256::MAX - U256::from(1)), None);
    }

    #[test]
    fn test_mul_div_all_max_inputs() {
        assert_eq!(mul_div(U256::MAX, U256::MAX, U256::MAX), Some(U256::MAX));
    }

    #[test]
    fn test_mul_div_specific_cases() {
        let half_q128 = Q128 / U256::from(2); // 0.5x
        let one_and_a_half_q128 = Q128 * U256::from(3) / U256::from(2); // 1.5x
        assert_eq!(
            mul_div(Q128, half_q128, one_and_a_half_q128),
            Some(Q128 / U256::from(3))
        );

        assert_eq!(
            mul_div(Q128, Q128 * U256::from(35), Q128 * U256::from(8)),
            Some(Q128 * U256::from(4375) / U256::from(1000))
        );
        
        assert_eq!(
            mul_div(Q128, Q128 * U256::from(1000), Q128 * U256::from(3000)),
            Some(Q128 / U256::from(3))
        );
    }

    #[test]
    fn test_mul_div_rounding_up_reverts() {
        assert_eq!(mul_div_rounding_up(Q128, U256::from(5), U256::ZERO), None);
        assert_eq!(mul_div_rounding_up(U256::MAX, U256::MAX, U256::MAX - U256::from(1)), None);
    }

    #[test]
    fn test_mul_div_rounding_up_all_max_inputs() {
        assert_eq!(mul_div_rounding_up(U256::MAX, U256::MAX, U256::MAX), Some(U256::MAX));
    }
    
    #[test]
    fn test_mul_div_rounding_up_specific_cases() {
        let half_q128 = Q128 / U256::from(2);
        let one_and_a_half_q128 = Q128 * U256::from(3) / U256::from(2);
        // (1 * 0.5) / 1.5 = 1/3, rounds up
        assert_eq!(
            mul_div_rounding_up(Q128, half_q128, one_and_a_half_q128),
            Some(Q128 / U256::from(3) + U256::from(1))
        );

        // (1 * 35) / 8 = 4.375, no rounding up needed for this precision
        assert_eq!(
            mul_div_rounding_up(Q128, Q128 * U256::from(35), Q128 * U256::from(8)),
            Some(Q128 * U256::from(4375) / U256::from(1000))
        );

        // (1 * 1000) / 3000 = 1/3, rounds up
        assert_eq!(
            mul_div_rounding_up(Q128, Q128 * U256::from(1000), Q128 * U256::from(3000)),
            Some(Q128 / U256::from(3) + U256::from(1))
        );
    }
}
