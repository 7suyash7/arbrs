use super::constants::{MAX_SQRT_RATIO, MAX_TICK, MIN_SQRT_RATIO};
use crate::errors::ArbRsError::UniswapV3MathError;
use crate::math::v3::constants::{SQRT_10001, TICK_HIGH, TICK_LOW};
use alloy_primitives::{I256, U256};
use std::ops::{Neg, Shl, Shr, BitOr};

const U256_1: U256 = U256::from_limbs([1, 0, 0, 0]);
const U256_2: U256 = U256::from_limbs([2, 0, 0, 0]);
const U256_3: U256 = U256::from_limbs([3, 0, 0, 0]);
const U256_4: U256 = U256::from_limbs([4, 0, 0, 0]);
const U256_5: U256 = U256::from_limbs([5, 0, 0, 0]);
const U256_6: U256 = U256::from_limbs([6, 0, 0, 0]);
const U256_7: U256 = U256::from_limbs([7, 0, 0, 0]);
const U256_8: U256 = U256::from_limbs([8, 0, 0, 0]);
const U256_15: U256 = U256::from_limbs([15, 0, 0, 0]);
const U256_16: U256 = U256::from_limbs([16, 0, 0, 0]);
const U256_32: U256 = U256::from_limbs([32, 0, 0, 0]);
const U256_64: U256 = U256::from_limbs([64, 0, 0, 0]);
const U256_127: U256 = U256::from_limbs([127, 0, 0, 0]);
const U256_128: U256 = U256::from_limbs([128, 0, 0, 0]);
const U256_255: U256 = U256::from_limbs([255, 0, 0, 0]);

const U256_256: U256 = U256::from_limbs([256, 0, 0, 0]);
const U256_512: U256 = U256::from_limbs([512, 0, 0, 0]);
const U256_1024: U256 = U256::from_limbs([1024, 0, 0, 0]);
const U256_2048: U256 = U256::from_limbs([2048, 0, 0, 0]);
const U256_4096: U256 = U256::from_limbs([4096, 0, 0, 0]);
const U256_8192: U256 = U256::from_limbs([8192, 0, 0, 0]);
const U256_16384: U256 = U256::from_limbs([16384, 0, 0, 0]);
const U256_32768: U256 = U256::from_limbs([32768, 0, 0, 0]);
const U256_65536: U256 = U256::from_limbs([65536, 0, 0, 0]);
const U256_131072: U256 = U256::from_limbs([131072, 0, 0, 0]);
const U256_262144: U256 = U256::from_limbs([262144, 0, 0, 0]);
const U256_524288: U256 = U256::from_limbs([524288, 0, 0, 0]);

/// Calculates sqrt(1.0001^tick) * 2^96 from a given tick.
pub fn get_sqrt_ratio_at_tick(tick: i32) -> Result<U256, crate::ArbRsError> {
    let abs_tick = if tick < 0 {
        U256::from(tick.neg())
    } else {
        U256::from(tick)
    };

    if abs_tick > MAX_TICK {
        return Err(UniswapV3MathError(String::from("tick out of bounds")));
    }

    let mut ratio = if abs_tick & (U256_1) != U256::ZERO {
        U256::from_limbs([12262481743371124737, 18445821805675392311, 0, 0])
    } else {
        U256::from_limbs([0, 0, 1, 0])
    };

    if !(abs_tick & U256_2).is_zero() {
        ratio = (ratio * U256::from_limbs([6459403834229662010, 18444899583751176498, 0, 0])) >> 128
    }
    if !(abs_tick & U256_4).is_zero() {
        ratio =
            (ratio * U256::from_limbs([17226890335427755468, 18443055278223354162, 0, 0])) >> 128
    }
    if !(abs_tick & U256_8).is_zero() {
        ratio = (ratio * U256::from_limbs([2032852871939366096, 18439367220385604838, 0, 0])) >> 128
    }
    if !(abs_tick & U256_16).is_zero() {
        ratio =
            (ratio * U256::from_limbs([14545316742740207172, 18431993317065449817, 0, 0])) >> 128
    }
    if !(abs_tick & U256_32).is_zero() {
        ratio = (ratio * U256::from_limbs([5129152022828963008, 18417254355718160513, 0, 0])) >> 128
    }
    if !(abs_tick & U256_64).is_zero() {
        ratio = (ratio * U256::from_limbs([4894419605888772193, 18387811781193591352, 0, 0])) >> 128
    }
    if !(abs_tick & U256_128).is_zero() {
        ratio = (ratio * U256::from_limbs([1280255884321894483, 18329067761203520168, 0, 0])) >> 128
    }
    if !(abs_tick & U256_256).is_zero() {
        ratio =
            (ratio * U256::from_limbs([15924666964335305636, 18212142134806087854, 0, 0])) >> 128
    }
    if !(abs_tick & U256_512).is_zero() {
        ratio = (ratio * U256::from_limbs([8010504389359918676, 17980523815641551639, 0, 0])) >> 128
    }
    if !(abs_tick & U256_1024).is_zero() {
        ratio =
            (ratio * U256::from_limbs([10668036004952895731, 17526086738831147013, 0, 0])) >> 128
    }
    if !(abs_tick & U256_2048).is_zero() {
        ratio = (ratio * U256::from_limbs([4878133418470705625, 16651378430235024244, 0, 0])) >> 128
    }
    if !(abs_tick & U256_4096).is_zero() {
        ratio = (ratio * U256::from_limbs([9537173718739605541, 15030750278693429944, 0, 0])) >> 128
    }
    if !(abs_tick & U256_8192).is_zero() {
        ratio = (ratio * U256::from_limbs([9972618978014552549, 12247334978882834399, 0, 0])) >> 128
    }
    if !(abs_tick & U256_16384).is_zero() {
        ratio = (ratio * U256::from_limbs([10428997489610666743, 8131365268884726200, 0, 0])) >> 128
    }
    if !(abs_tick & U256_32768).is_zero() {
        ratio = (ratio * U256::from_limbs([9305304367709015974, 3584323654723342297, 0, 0])) >> 128
    }
    if !(abs_tick & U256_65536).is_zero() {
        ratio = (ratio * U256::from_limbs([14301143598189091785, 696457651847595233, 0, 0])) >> 128
    }
    if !(abs_tick & U256_131072).is_zero() {
        ratio = (ratio * U256::from_limbs([7393154844743099908, 26294789957452057, 0, 0])) >> 128
    }
    if !(abs_tick & U256_262144).is_zero() {
        ratio = (ratio * U256::from_limbs([2209338891292245656, 37481735321082, 0, 0])) >> 128
    }
    if !(abs_tick & U256_524288).is_zero() {
        ratio = (ratio * U256::from_limbs([10518117631919034274, 76158723, 0, 0])) >> 128
    }

    if tick > 0 {
        ratio = U256::MAX / ratio;
    }

    let final_ratio = (ratio >> 32) + if ratio.wrapping_rem(U256::from(1) << 32).is_zero() { U256::ZERO } else { U256_1 };

    Ok(final_ratio)
}

/// Calculates the tick from a given sqrt(price) * 2^96.
pub fn get_tick_at_sqrt_ratio(sqrt_price_x_96: U256) -> Result<i32, crate::ArbRsError> {
    if !(sqrt_price_x_96 >= MIN_SQRT_RATIO && sqrt_price_x_96 <= MAX_SQRT_RATIO) {
        return Err(UniswapV3MathError(String::from("R")));
    }

    let ratio: U256 = sqrt_price_x_96 << 32;
    let mut r = ratio;
    let mut msb = U256::ZERO;

    let mut f = if r > U256::from_limbs([18446744073709551615, 18446744073709551615, 0, 0]) { U256_1.shl(U256_7) } else { U256::ZERO };
    msb = msb.bitor(f); r = r.shr(f);
    f = if r > U256::from_limbs([18446744073709551615, 0, 0, 0]) { U256_1.shl(U256_6) } else { U256::ZERO };
    msb = msb.bitor(f); r = r.shr(f);
    f = if r > U256::from_limbs([4294967295, 0, 0, 0]) { U256_1.shl(U256_5) } else { U256::ZERO };
    msb = msb.bitor(f); r = r.shr(f);
    f = if r > U256::from_limbs([65535, 0, 0, 0]) { U256_1.shl(U256_4) } else { U256::ZERO };
    msb = msb.bitor(f); r = r.shr(f);
    f = if r > U256_255 { U256_1.shl(U256_3) } else { U256::ZERO };
    msb = msb.bitor(f); r = r.shr(f);
    f = if r > U256_15 { U256_1.shl(U256_2) } else { U256::ZERO };
    msb = msb.bitor(f); r = r.shr(f);
    f = if r > U256_3 { U256_1.shl(U256_1) } else { U256::ZERO };
    msb = msb.bitor(f); r = r.shr(f);
    f = if r > U256_1 { U256_1 } else { U256::ZERO };
    msb = msb.bitor(f);

    r = if msb >= U256_128 { ratio.shr(msb - U256_127) } else { ratio.shl(U256_127 - msb) };

    let mut log_2: I256 = (I256::from_raw(msb) - I256::from_raw(U256_128)).shl(64);

    for i in (0..=13).rev() {
        r = r.overflowing_mul(r).0 >> 127;
        let f = r >> 128;
        log_2 = log_2.bitor(I256::from_raw(f << (50 + i)));
        r >>= f;
    }

    let log_sqrt10001 = log_2.wrapping_mul(SQRT_10001);
    let tick_low = ((log_sqrt10001 - TICK_LOW) >> 128_u8).low_i32();
    let tick_high = ((log_sqrt10001 + TICK_HIGH) >> 128_u8).low_i32();

    let tick = if tick_low == tick_high {
        tick_low
    } else if get_sqrt_ratio_at_tick(tick_high)? <= sqrt_price_x_96 {
        tick_high
    } else {
        tick_low
    };

    Ok(tick)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::v3::constants::{MIN_TICK, MAX_TICK};
    use std::str::FromStr;
    use crate::errors::ArbRsError;

    #[test]
    fn test_get_sqrt_ratio_at_tick_bounds() {
        assert!(matches!(get_sqrt_ratio_at_tick(MIN_TICK - 1), Err(ArbRsError::UniswapV3MathError(_))));
        assert!(matches!(get_sqrt_ratio_at_tick(MAX_TICK + 1), Err(ArbRsError::UniswapV3MathError(_))));
    }

    #[test]
    fn test_get_sqrt_ratio_at_tick_values() {
        assert_eq!(get_sqrt_ratio_at_tick(MIN_TICK).unwrap(), MIN_SQRT_RATIO, "sqrt ratio at min incorrect");
        assert_eq!(get_sqrt_ratio_at_tick(MIN_TICK + 1).unwrap(), U256::from(4295343490u64), "sqrt ratio at min + 1 incorrect");
        assert_eq!(get_sqrt_ratio_at_tick(MAX_TICK - 1).unwrap(), U256::from_str("1461373636630004318706518188784493106690254656249").unwrap(), "sqrt ratio at max - 1 incorrect");
        assert_eq!(get_sqrt_ratio_at_tick(MAX_TICK).unwrap(), MAX_SQRT_RATIO, "sqrt ratio at max incorrect");

        assert_eq!(get_sqrt_ratio_at_tick(50).unwrap(), U256::from(79426470787362580746886972461u128), "sqrt ratio at 50 incorrect");
        assert_eq!(get_sqrt_ratio_at_tick(100).unwrap(), U256::from(79625275426524748796330556128u128), "sqrt ratio at 100 incorrect");
        assert_eq!(get_sqrt_ratio_at_tick(250).unwrap(), U256::from(80224679980005306637834519095u128), "sqrt ratio at 250 incorrect");
        assert_eq!(get_sqrt_ratio_at_tick(500).unwrap(), U256::from(81233731461783161732293370115u128), "sqrt ratio at 500 incorrect");
        assert_eq!(get_sqrt_ratio_at_tick(1000).unwrap(), U256::from(83290069058676223003182343270u128), "sqrt ratio at 1000 incorrect");
        assert_eq!(get_sqrt_ratio_at_tick(2500).unwrap(), U256::from(89776708723587163891445672585u128), "sqrt ratio at 2500 incorrect");
        assert_eq!(get_sqrt_ratio_at_tick(3000).unwrap(), U256::from(92049301871182272007977902845u128), "sqrt ratio at 3000 incorrect");
        assert_eq!(get_sqrt_ratio_at_tick(4000).unwrap(), U256::from(96768528593268422080558758223u128), "sqrt ratio at 4000 incorrect");
        assert_eq!(get_sqrt_ratio_at_tick(5000).unwrap(), U256::from(101729702841318637793976746270u128), "sqrt ratio at 5000 incorrect");
        assert_eq!(get_sqrt_ratio_at_tick(50000).unwrap(), U256::from(965075977353221155028623082916u128), "sqrt ratio at 50000 incorrect");
        assert_eq!(get_sqrt_ratio_at_tick(150000).unwrap(), U256::from(143194173941309278083010301478497u128), "sqrt ratio at 150000 incorrect");
        assert_eq!(get_sqrt_ratio_at_tick(250000).unwrap(), U256::from(21246587762933397357449903968194344u128), "sqrt ratio at 250000 incorrect");
        assert_eq!(get_sqrt_ratio_at_tick(500000).unwrap(), U256::from_str("5697689776495288729098254600827762987878").unwrap(), "sqrt ratio at 500000 incorrect");
        assert_eq!(get_sqrt_ratio_at_tick(738203).unwrap(), U256::from_str("847134979253254120489401328389043031315994541").unwrap(), "sqrt ratio at 738203 incorrect");
    }

    #[test]
    fn test_get_tick_at_sqrt_ratio() {
        assert!(matches!(get_tick_at_sqrt_ratio(MIN_SQRT_RATIO - U256_1), Err(ArbRsError::UniswapV3MathError(_))));
        // Test that it accepts MAX_SQRT_RATIO
        assert!(get_tick_at_sqrt_ratio(MAX_SQRT_RATIO).is_ok());

        assert_eq!(get_tick_at_sqrt_ratio(MIN_SQRT_RATIO).unwrap(), MIN_TICK, "tick at min ratio incorrect");
        assert_eq!(get_tick_at_sqrt_ratio(U256::from_str("4295343490").unwrap()).unwrap(), MIN_TICK + 1, "tick at min ratio + 1 incorrect");
        assert_eq!(get_tick_at_sqrt_ratio(MAX_SQRT_RATIO).unwrap(), MAX_TICK, "tick at max ratio incorrect");
    }

    #[test]
    fn test_tick_and_ratio_roundtrip() {
        for tick in [MIN_TICK, MIN_TICK + 1, -12345, 0, 12345, MAX_TICK - 1, MAX_TICK].iter() {
            let sqrt_ratio = get_sqrt_ratio_at_tick(*tick).unwrap();
            let derived_tick = get_tick_at_sqrt_ratio(sqrt_ratio).unwrap();
            assert!((derived_tick - *tick).abs() <= 1, "Roundtrip failed for tick {}", tick);
        }
    }
}