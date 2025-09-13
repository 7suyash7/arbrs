use alloy_primitives::U256;

pub fn div_rounding_up(x: U256, y: U256) -> U256 {
    if y.is_zero() {
        panic!("attempt to divide by zero");
    }
    if x.is_zero() {
        return U256::ZERO;
    }
    (x - U256::from(1)) / y + U256::from(1)
}