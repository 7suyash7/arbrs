use alloy_primitives::U256;
use std::str::FromStr;

// Constants defining the range of valid ticks and corresponding sqrt prices.
pub const MIN_TICK: i32 = -887272;
pub const MAX_TICK: i32 = 887272;

// MIN_SQRT_RATIO = sqrt(1.0001^MIN_TICK) * 2^96
lazy_static::lazy_static! {
    pub static ref MIN_SQRT_RATIO: U256 = U256::from_str("4295128739").unwrap();
    pub static ref MAX_SQRT_RATIO: U256 = U256::from_str("1461446703485210103287273052203988822378723970342").unwrap();
}

/// Calculates sqrt(1.0001^tick) * 2^96 from a given tick.
/// Replicates the logic from Uniswap V3's TickMath library.
/// Panics if the tick is outside the valid range [MIN_TICK, MAX_TICK].
pub fn get_sqrt_ratio_at_tick(tick: i32) -> Option<U256> {
    if !(MIN_TICK..=MAX_TICK).contains(&tick) {
        return None; // Tick out of bounds
    }

    let abs_tick = tick.unsigned_abs() as u32;

    let mut ratio = if abs_tick & 0x1 != 0 {
        U256::from_str("0xfffcb933bd6fad37aa2d162d1a594001").unwrap()
    } else {
        U256::from_str("0x100000000000000000000000000000000").unwrap()
    };

    if abs_tick & 0x2 != 0 {
        ratio = (ratio * U256::from_str("0xfff97272373d413e734ad72f5bc06a24").unwrap()) >> 128;
    }
    if abs_tick & 0x4 != 0 {
        ratio = (ratio * U256::from_str("0xfff2e50f5f656932ef12357cf3c7fdcc").unwrap()) >> 128;
    }
    if abs_tick & 0x8 != 0 {
        ratio = (ratio * U256::from_str("0xffe5caca7e10e4e61c3624eaa0941cd0").unwrap()) >> 128;
    }
    if abs_tick & 0x10 != 0 {
        ratio = (ratio * U256::from_str("0xffcb9843d60f6159c9db58835c926644").unwrap()) >> 128;
    }
    if abs_tick & 0x20 != 0 {
        ratio = (ratio * U256::from_str("0xff973b41fa98c081472e6896dfb254c0").unwrap()) >> 128;
    }
    if abs_tick & 0x40 != 0 {
        ratio = (ratio * U256::from_str("0xff2ea16466c96a3843ec78b326b52861").unwrap()) >> 128;
    }
    if abs_tick & 0x80 != 0 {
        ratio = (ratio * U256::from_str("0xfe5dee046a99a2a811c461f1969c3053").unwrap()) >> 128;
    }
    if abs_tick & 0x100 != 0 {
        ratio = (ratio * U256::from_str("0xfcbe86c7900a88aedc4ac3b14a926ef1").unwrap()) >> 128;
    }
    if abs_tick & 0x200 != 0 {
        ratio = (ratio * U256::from_str("0xf987a7253ac413176f2b074cf7f60990").unwrap()) >> 128;
    }
    if abs_tick & 0x400 != 0 {
        ratio = (ratio * U256::from_str("0xf3392b0822b70005940c7a398e4b70f3").unwrap()) >> 128;
    }
    if abs_tick & 0x800 != 0 {
        ratio = (ratio * U256::from_str("0xe7159475a21d5de9b84315f068218bac").unwrap()) >> 128;
    }
    if abs_tick & 0x1000 != 0 {
        ratio = (ratio * U256::from_str("0xd097f3bdfd2022b8845ad8f792aa5825").unwrap()) >> 128;
    }
    if abs_tick & 0x2000 != 0 {
        ratio = (ratio * U256::from_str("0xa9f746462d870fdf8a65dc1f90e061e5").unwrap()) >> 128;
    }
    if abs_tick & 0x4000 != 0 {
        ratio = (ratio * U256::from_str("0x70d869a156d2a1b890bb3df62baf32f7").unwrap()) >> 128;
    }
    if abs_tick & 0x8000 != 0 {
        ratio = (ratio * U256::from_str("0x31be135f97d08fd981231505542fcfa6").unwrap()) >> 128;
    }
    if abs_tick & 0x10000 != 0 {
        ratio = (ratio * U256::from_str("0x9aa508b5b7a84e1c677de54f2e99bc9").unwrap()) >> 128;
    }
    if abs_tick & 0x20000 != 0 {
        ratio = (ratio * U256::from_str("0x5d62a50a505f0490c918b614531de243").unwrap()) >> 128;
    }
    if abs_tick & 0x40000 != 0 {
        ratio = (ratio * U256::from_str("0x2216e584f5fa1ea926041ad88d2600eb").unwrap()) >> 128;
    }
    if abs_tick & 0x80000 != 0 {
        ratio = (ratio * U256::from_str("0x48a170391f7dc42444e808192b8f741").unwrap()) >> 128;
    }

    if tick < 0 {
        // Calculate reciprocal: 1 / ratio = (2^128 * 2^128) / ratio = 2^256 / ratio
        // We use U256::MAX for 2^256 - 1, which approximates 2^256 for integer division.
        ratio = (U256::MAX / ratio) + U256::from(1);
    }

    Some(ratio)
}

/// Calculates the tick from a given sqrt(price) * 2^96.
/// Replicates the logic from Uniswap V3's TickMath library.
pub fn get_tick_at_sqrt_ratio(sqrt_ratio_x96: U256) -> Option<i32> {
    if !(*MIN_SQRT_RATIO <= sqrt_ratio_x96 && sqrt_ratio_x96 <= *MAX_SQRT_RATIO) {
        return None; // sqrt_ratio out of bounds
    }

    let ratio = sqrt_ratio_x96 << 32;

    let mut r = ratio;
    let mut msb = 0;

    let f = |i, r: U256| -> U256 {
        (r * r * r * r) >> (128 - i)
    };

    if r >= (U256::from(1) << 128) { r >>= 128; msb += 128; }
    if r >= (U256::from(1) << 64) { r >>= 64; msb += 64; }
    if r >= (U256::from(1) << 32) { r >>= 32; msb += 32; }
    if r >= (U256::from(1) << 16) { r >>= 16; msb += 16; }
    if r >= (U256::from(1) << 8) { r >>= 8; msb += 8; }
    if r >= (U256::from(1) << 4) { r >>= 4; msb += 4; }
    if r >= (U256::from(1) << 2) { r >>= 2; msb += 2; }
    if r >= (U256::from(1) << 1) { msb += 1; }

    let mut log_2: U256 = (U256::from(msb) - U256::from(96)) << 64;
    let log_10001: U256 = U256::from_str("2557380163388334138478235652933391151").unwrap();

    let mut r = ratio >> msb;

    if r >= U256::from_str("0x1ffffffffffffffffffffffffffffffff").unwrap() { log_2 += f(8,r); }
    r <<= 1;
    if r >= U256::from_str("0x1ffffffffffffffffffffffffffffffff").unwrap() { log_2 += f(7,r); }
    r <<= 1;
    if r >= U256::from_str("0x1ffffffffffffffffffffffffffffffff").unwrap() { log_2 += f(6,r); }
    r <<= 1;
    if r >= U256::from_str("0x1ffffffffffffffffffffffffffffffff").unwrap() { log_2 += f(5,r); }
    r <<= 1;
    if r >= U256::from_str("0x1ffffffffffffffffffffffffffffffff").unwrap() { log_2 += f(4,r); }
    r <<= 1;
    if r >= U256::from_str("0x1ffffffffffffffffffffffffffffffff").unwrap() { log_2 += f(3,r); }
    r <<= 1;
    if r >= U256::from_str("0x1ffffffffffffffffffffffffffffffff").unwrap() { log_2 += f(2,r); }
    r <<= 1;
    if r >= U256::from_str("0x1ffffffffffffffffffffffffffffffff").unwrap() { log_2 += f(1,r); }

    let tick_low = (log_2 / log_10001).to::<i128>();
    let tick_high = ((log_2 + log_10001 - U256::from(1)) / log_10001).to::<i128>();

    if tick_low == tick_high {
        return Some(tick_low as i32);
    }
    
    let sqrt_ratio_at_tick_low = get_sqrt_ratio_at_tick(tick_low as i32).unwrap();
    if sqrt_ratio_at_tick_low <= sqrt_ratio_x96 {
        Some(tick_low as i32)
    } else {
        Some(tick_high as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_sqrt_ratio_at_tick_bounds() {
        assert_eq!(get_sqrt_ratio_at_tick(MIN_TICK - 1), None);
        assert_eq!(get_sqrt_ratio_at_tick(MAX_TICK + 1), None);
    }

    #[test]
    fn test_get_sqrt_ratio_at_tick_min_max() {
        assert_eq!(get_sqrt_ratio_at_tick(MIN_TICK).unwrap(), *MIN_SQRT_RATIO);
        assert_eq!(get_sqrt_ratio_at_tick(MAX_TICK).unwrap(), *MAX_SQRT_RATIO);
    }

    #[test]
    fn test_get_sqrt_ratio_at_tick_specific_values() {
        assert_eq!(get_sqrt_ratio_at_tick(MIN_TICK + 1).unwrap(), U256::from(4295343490_u128));
        assert_eq!(
            get_sqrt_ratio_at_tick(MAX_TICK - 1).unwrap(),
            U256::from_str("1461373636630004318706518188784493106690254656249").unwrap()
        );
    }
    
    #[test]
    fn test_get_tick_at_sqrt_ratio_bounds() {
        assert_eq!(get_tick_at_sqrt_ratio(*MIN_SQRT_RATIO - U256::from(1)), None);
        assert_eq!(get_tick_at_sqrt_ratio(*MAX_SQRT_RATIO), None); // MAX_SQRT_RATIO is not inclusive
    }

    #[test]
    fn test_get_tick_at_sqrt_ratio_min_max() {
        assert_eq!(get_tick_at_sqrt_ratio(*MIN_SQRT_RATIO).unwrap(), MIN_TICK);
        assert_eq!(get_tick_at_sqrt_ratio(*MAX_SQRT_RATIO - U256::from(1)).unwrap(), MAX_TICK - 1);
    }
    
    #[test]
    fn test_get_tick_at_sqrt_ratio_specific_values() {
        assert_eq!(get_tick_at_sqrt_ratio(U256::from(4295343490_u128)).unwrap(), MIN_TICK + 1);
        assert_eq!(
            get_tick_at_sqrt_ratio(U256::from_str("1461373636630004318706518188784493106690254656249").unwrap()).unwrap(),
            MAX_TICK - 1
        );
    }
    
    #[test]
    fn test_tick_and_ratio_roundtrip() {
        for tick in [MIN_TICK, MIN_TICK + 1, -12345, 0, 12345, MAX_TICK - 1, MAX_TICK].iter() {
            let sqrt_ratio = get_sqrt_ratio_at_tick(*tick).unwrap();
            let derived_tick = get_tick_at_sqrt_ratio(sqrt_ratio).unwrap();
            assert_eq!(derived_tick, *tick);
        }
    }
}
