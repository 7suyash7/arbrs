use alloy_primitives::U256;

/// Calculates the integer square root of a U256.
/// Uses the Babylonian method for iterative approximation.
pub fn sqrt(x: U256) -> U256 {
    if x.is_zero() {
        return U256::ZERO;
    }
    // Initial guess is the nearest power of 2
    let mut z = U256::from(1) << (x.leading_zeros() / 2);
    let mut y = z;
    loop {
        y = z;
        z = (x / z + z) >> 1; // z = (x/z + z) / 2
        if z >= y {
            break;
        }
    }
    y
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sqrt() {
        assert_eq!(sqrt(U256::from(0)), U256::from(0));
        assert_eq!(sqrt(U256::from(1)), U256::from(1));
        assert_eq!(sqrt(U256::from(4)), U256::from(2));
        assert_eq!(sqrt(U256::from(16)), U256::from(4));
        assert_eq!(sqrt(U256::from(17)), U256::from(4)); // floor
        assert_eq!(sqrt(U256::MAX).to_string(), "340282366920938463463374607431768211455");
    }
}