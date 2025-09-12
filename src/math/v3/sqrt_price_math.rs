use super::full_math::{mul_div, mul_div_rounding_up};
use alloy_primitives::{U256, U512};

// This is a more robust implementation that correctly mirrors the fixed-point
// arithmetic logic found in mature Uniswap V3 libraries.

fn div_rounding_up(a: U256, b: U256) -> Option<U256> {
    if a.is_zero() {
        return Some(U256::ZERO);
    }
    if b.is_zero() {
        return None;
    }
    Some((a - U256::from(1)) / b + U256::from(1))
}

fn get_next_sqrt_price_from_amount0_rounding_up(
    sqrt_p_x96: U256,
    liquidity: u128,
    amount: U256,
    add: bool,
) -> Option<U256> {
    if amount.is_zero() {
        return Some(sqrt_p_x96);
    }
    let liquidity_u256 = U256::from(liquidity);
    let numerator1 = liquidity_u256 << 96;

    if add {
        let product = amount * sqrt_p_x96;
        if product / amount == sqrt_p_x96 {
            let denominator = numerator1 + product;
            if denominator >= numerator1 {
                return mul_div_rounding_up(numerator1, sqrt_p_x96, denominator);
            }
        }
        let denominator = div_rounding_up(numerator1, sqrt_p_x96)? + amount;
        return div_rounding_up(numerator1, denominator);
    } else {
        let product = amount * sqrt_p_x96;
        if product / amount == sqrt_p_x96 && numerator1 > product {
            let denominator = numerator1 - product;
            return mul_div_rounding_up(numerator1, sqrt_p_x96, denominator);
        }
        None
    }
}

fn get_next_sqrt_price_from_amount1_rounding_down(
    sqrt_p_x96: U256,
    liquidity: u128,
    amount: U256,
    add: bool,
) -> Option<U256> {
    let liquidity_u256 = U256::from(liquidity);
    if add {
        let quotient = mul_div(amount, U256::from(1) << 96, liquidity_u256)?;
        Some(sqrt_p_x96 + quotient)
    } else {
        let quotient = mul_div_rounding_up(amount, U256::from(1) << 96, liquidity_u256)?;
        if sqrt_p_x96 <= quotient {
            return None;
        }
        Some(sqrt_p_x96 - quotient)
    }
}

pub fn get_next_sqrt_price_from_input(
    sqrt_p_x96: U256,
    liquidity: u128,
    amount_in: U256,
    zero_for_one: bool,
) -> Option<U256> {
    if sqrt_p_x96.is_zero() || liquidity == 0 {
        return None;
    }
    if zero_for_one {
        get_next_sqrt_price_from_amount0_rounding_up(sqrt_p_x96, liquidity, amount_in, true)
    } else {
        get_next_sqrt_price_from_amount1_rounding_down(sqrt_p_x96, liquidity, amount_in, true)
    }
}

pub fn get_next_sqrt_price_from_output(
    sqrt_p_x96: U256,
    liquidity: u128,
    amount_out: U256,
    zero_for_one: bool,
) -> Option<U256> {
    if sqrt_p_x96.is_zero() || liquidity == 0 {
        return None;
    }
    if zero_for_one {
        get_next_sqrt_price_from_amount1_rounding_down(sqrt_p_x96, liquidity, amount_out, false)
    } else {
        get_next_sqrt_price_from_amount0_rounding_up(sqrt_p_x96, liquidity, amount_out, false)
    }
}

pub fn get_amount0_delta(
    sqrt_ratio_a_x96: U256,
    sqrt_ratio_b_x96: U256,
    liquidity: u128,
    round_up: bool,
) -> Option<U256> {
    let (mut sqrt_ratio_a_x96, mut sqrt_ratio_b_x96) = (sqrt_ratio_a_x96, sqrt_ratio_b_x96);
    if sqrt_ratio_a_x96 > sqrt_ratio_b_x96 {
        std::mem::swap(&mut sqrt_ratio_a_x96, &mut sqrt_ratio_b_x96);
    }

    let liquidity_u256 = U256::from(liquidity);
    let numerator1 = liquidity_u256 << 96;
    let numerator2 = sqrt_ratio_b_x96 - sqrt_ratio_a_x96;

    if sqrt_ratio_a_x96.is_zero() {
        return None;
    }

    if round_up {
        let res = mul_div_rounding_up(numerator1, numerator2, sqrt_ratio_b_x96)?;
        div_rounding_up(res, sqrt_ratio_a_x96)
    } else {
        let res = mul_div(numerator1, numerator2, sqrt_ratio_b_x96)?;
        Some(res / sqrt_ratio_a_x96)
    }
}

pub fn get_amount1_delta(
    sqrt_ratio_a_x96: U256,
    sqrt_ratio_b_x96: U256,
    liquidity: u128,
    round_up: bool,
) -> Option<U256> {
    let (mut sqrt_ratio_a_x96, mut sqrt_ratio_b_x96) = (sqrt_ratio_a_x96, sqrt_ratio_b_x96);
    if sqrt_ratio_a_x96 > sqrt_ratio_b_x96 {
        std::mem::swap(&mut sqrt_ratio_a_x96, &mut sqrt_ratio_b_x96);
    }

    let liquidity = U256::from(liquidity);
    let sqrt_diff = sqrt_ratio_b_x96 - sqrt_ratio_a_x96;

    if round_up {
        mul_div_rounding_up(liquidity, sqrt_diff, U256::from(1) << 96)
    } else {
        mul_div(liquidity, sqrt_diff, U256::from(1) << 96)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::v3::utils::sqrt;
    use alloy_primitives::U256;
    use std::str::FromStr;

    fn e18(n: u64) -> U256 {
        U256::from(n) * U256::from(10).pow(U256::from(18))
    }

    fn encode_price_sqrt(reserve1: u128, reserve0: u128) -> U256 {
        let r1 = U256::from(reserve1);
        let r0 = U256::from(reserve0);
        sqrt(r1 * (U256::from(1) << 192) / r0)
    }

    #[test]
    fn test_get_amount_0_delta_simple() {
        let liquidity = e18(1).to::<u128>();
        let sqrt_p_a = encode_price_sqrt(1, 1);
        let sqrt_p_b = encode_price_sqrt(121, 100);

        let amount0_up = get_amount0_delta(sqrt_p_a, sqrt_p_b, liquidity, true).unwrap();
        assert_eq!(amount0_up, U256::from_str("90909090909090910").unwrap());

        let amount0_down = get_amount0_delta(sqrt_p_a, sqrt_p_b, liquidity, false).unwrap();
        assert_eq!(amount0_down, amount0_up - U256::from(1));
    }

    #[test]
    fn test_get_amount_1_delta_simple() {
        let liquidity = e18(1).to::<u128>();
        let sqrt_p_a = encode_price_sqrt(1, 1);
        let sqrt_p_b = encode_price_sqrt(121, 100);

        let amount1_up = get_amount1_delta(sqrt_p_a, sqrt_p_b, liquidity, true).unwrap();
        assert_eq!(amount1_up, U256::from_str("100000000000000000").unwrap());

        let amount1_down = get_amount1_delta(sqrt_p_a, sqrt_p_b, liquidity, false).unwrap();
        assert_eq!(amount1_down, amount1_up - U256::from(1));
    }

    #[test]
    fn test_get_next_sqrt_price_from_input_zero_liquidity() {
        let price = encode_price_sqrt(1, 1);
        assert_eq!(
            get_next_sqrt_price_from_input(price, 0, e18(1), true),
            None
        );
    }

    #[test]
    fn test_get_next_sqrt_price_from_input_zero_amount() {
        let price = encode_price_sqrt(1, 1);
        let liquidity = e18(1).to::<u128>();
        assert_eq!(
            get_next_sqrt_price_from_input(price, liquidity, U256::ZERO, true).unwrap(),
            price
        );
        assert_eq!(
            get_next_sqrt_price_from_input(price, liquidity, U256::ZERO, false).unwrap(),
            price
        );
    }

    #[test]
    fn test_get_next_sqrt_price_from_input_specific_cases() {
        let sqrt_q = get_next_sqrt_price_from_input(
            encode_price_sqrt(1, 1),
            e18(1).to::<u128>(),
            e18(1) / U256::from(10),
            false,
        )
        .unwrap();
        assert_eq!(
            sqrt_q,
            U256::from_str("87150978765690771352898345369").unwrap()
        );

        let sqrt_q2 = get_next_sqrt_price_from_input(
            encode_price_sqrt(1, 1),
            e18(1).to::<u128>(),
            e18(1) / U256::from(10),
            true,
        )
        .unwrap();
        assert_eq!(
            sqrt_q2,
            U256::from_str("72025602285694852357767227579").unwrap()
        );
    }
}