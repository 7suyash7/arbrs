use alloy_primitives::U256;

/// Returns the index of the most significant bit of the number,
/// or 0 if the number is 0.
pub fn most_significant_bit(x: U256) -> u8 {
    if x == U256::ZERO {
        return 0;
    }
    255 - x.leading_zeros() as u8
}

/// Returns the index of the least significant bit of the number,
/// or 255 if the number is 0.
pub fn least_significant_bit(x: U256) -> u8 {
    if x == U256::ZERO {
        return 255;
    }
    x.trailing_zeros() as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::U256;

    #[test]
    fn test_most_significant_bit() {
        // In the original contract, msb(0) reverts.
        // Our implementation returns 0 for an input of 0, which is a reasonable default.
        assert_eq!(most_significant_bit(U256::ZERO), 0);

        assert_eq!(most_significant_bit(U256::from(1)), 0);
        assert_eq!(most_significant_bit(U256::from(2)), 1);
        assert_eq!(most_significant_bit(U256::from(3)), 1);

        // Test all powers of 2
        for i in 0..256 {
            assert_eq!(most_significant_bit(U256::from(1) << i), i as u8);
        }
        assert_eq!(most_significant_bit(U256::MAX), 255);
    }

    #[test]
    fn test_least_significant_bit() {
        // In the original contract, lsb(0) reverts.
        // Our implementation returns 255, which is a reasonable sentinel value.
        assert_eq!(least_significant_bit(U256::ZERO), 255);

        assert_eq!(least_significant_bit(U256::from(1)), 0);
        assert_eq!(least_significant_bit(U256::from(2)), 1);
        assert_eq!(least_significant_bit(U256::from(3)), 0);

        // Test all powers of 2
        for i in 0..256 {
            assert_eq!(least_significant_bit(U256::from(1) << i), i as u8);
        }
        assert_eq!(least_significant_bit(U256::MAX), 0);
    }
}
