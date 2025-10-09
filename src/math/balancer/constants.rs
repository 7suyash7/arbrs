use alloy_primitives::U256;

// All fixed-point numbers are implemented with 18 decimals
pub const ONE: U256 = U256::from_limbs([1_000_000_000_000_000_000, 0, 0, 0]);
pub const TWO: U256 = U256::from_limbs([2_000_000_000_000_000_000, 0, 0, 0]);
pub const FOUR: U256 = U256::from_limbs([4_000_000_000_000_000_000, 0, 0, 0]);

// Used for pow calculations
pub const MAX_POW_RELATIVE_ERROR: U256 = U256::from_limbs([10000, 0, 0, 0]);
