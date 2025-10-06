use alloy_primitives::U256;

/// Converts a U256 into a f64, manually combining its limbs.
/// This is an approximation and will lose precision for very large numbers,
/// but is suitable for price calculations.
pub fn u256_to_f64(value: U256) -> f64 {
    let limbs = value.as_limbs();
    let mut result = 0.0;

    const TWO_POW_64: f64 = (1u64 << 63) as f64 * 2.0;

    result += limbs[3] as f64;
    result = result * TWO_POW_64 + (limbs[2] as f64);
    result = result * TWO_POW_64 + (limbs[1] as f64);
    result = result * TWO_POW_64 + (limbs[0] as f64);
    
    result
}