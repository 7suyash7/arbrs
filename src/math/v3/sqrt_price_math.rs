use crate::errors::ArbRsError;
use crate::math::v3::{
    full_math::{mul_div, mul_div_rounding_up},
    unsafe_math::div_rounding_up,
};
use alloy_primitives::{I256, U256};

pub const MAX_U160: U256 = U256::from_limbs([0, 0, 0, 1 << 32]); // 2^160
pub const Q96: U256 = U256::from_limbs([0, 1 << 32, 0, 0]); // 2^96

fn get_next_sqrt_price_from_amount_0_rounding_up(
    sqrt_price_x_96: U256,
    liquidity: u128,
    amount: U256,
    add: bool,
) -> Result<U256, ArbRsError> {
    if amount.is_zero() {
        return Ok(sqrt_price_x_96);
    }
    let numerator_1 = U256::from(liquidity) << 96;

    if add {
        let product = amount * sqrt_price_x_96;
        if product / amount == sqrt_price_x_96 {
            let denominator = numerator_1 + product;
            if denominator >= numerator_1 {
                return mul_div_rounding_up(numerator_1, sqrt_price_x_96, denominator)
                    .ok_or(ArbRsError::UniswapV3MathError("mul_div_rounding_up failed".into()));
            }
        }
        let denominator = div_rounding_up(numerator_1, sqrt_price_x_96) + amount;
        Ok(div_rounding_up(numerator_1, denominator))
    } else {
        let product = amount * sqrt_price_x_96;
        if product / amount == sqrt_price_x_96 && numerator_1 > product {
            let denominator = numerator_1 - product;
            mul_div_rounding_up(numerator_1, sqrt_price_x_96, denominator)
                .ok_or(ArbRsError::UniswapV3MathError("mul_div_rounding_up failed".into()))
        } else {
            Err(ArbRsError::UniswapV3MathError("Paus".into()))
        }
    }
}

fn get_next_sqrt_price_from_amount_1_rounding_down(
    sqrt_price_x_96: U256,
    liquidity: u128,
    amount: U256,
    add: bool,
) -> Result<U256, ArbRsError> {
    let liquidity = U256::from(liquidity);
    if add {
        let quotient = mul_div(amount, Q96, liquidity)
            .ok_or(ArbRsError::UniswapV3MathError("mul_div failed".into()))?;
        let next_sqrt_price = sqrt_price_x_96 + quotient;
        if next_sqrt_price > MAX_U160 {
            return Err(ArbRsError::UniswapV3MathError("R".into()));
        }
        Ok(next_sqrt_price)
    } else {
        let quotient = mul_div_rounding_up(amount, Q96, liquidity)
            .ok_or(ArbRsError::UniswapV3MathError("mul_div_rounding_up failed".into()))?;
        if sqrt_price_x_96 <= quotient {
            return Err(ArbRsError::UniswapV3MathError("R".into()));
        }
        Ok(sqrt_price_x_96 - quotient)
    }
}

pub fn get_next_sqrt_price_from_input(
    sqrt_price: U256,
    liquidity: u128,
    amount_in: U256,
    zero_for_one: bool,
) -> Result<U256, ArbRsError> {
    if liquidity == 0 { return Err(ArbRsError::UniswapV3MathError("L".into())); }
    if zero_for_one {
        get_next_sqrt_price_from_amount_0_rounding_up(sqrt_price, liquidity, amount_in, true)
    } else {
        get_next_sqrt_price_from_amount_1_rounding_down(sqrt_price, liquidity, amount_in, true)
    }
}

pub fn get_next_sqrt_price_from_output(
    sqrt_p_x96: U256,
    liquidity: u128,
    amount_out: U256,
    zero_for_one: bool,
) -> Result<U256, ArbRsError> {
    if liquidity == 0 { return Err(ArbRsError::UniswapV3MathError("L".into())); }
    let liquidity_u256 = U256::from(liquidity);

    if zero_for_one {
        let quotient = mul_div_rounding_up(amount_out, Q96, liquidity_u256)
            .ok_or(ArbRsError::UniswapV3MathError("mul_div_rounding_up failed".into()))?;
        if sqrt_p_x96 <= quotient {
             return Err(ArbRsError::UniswapV3MathError("R".into()));
        }
        Ok(sqrt_p_x96 - quotient)
    } else {
        let product = amount_out * sqrt_p_x96;
        let denominator = (liquidity_u256 << 96) - product;
        mul_div_rounding_up(liquidity_u256 << 96, sqrt_p_x96, denominator)
            .ok_or(ArbRsError::UniswapV3MathError("mul_div_rounding_up failed".into()))
    }
}

pub fn get_amount0_delta(
    mut sqrt_ratio_a_x96: U256,
    mut sqrt_ratio_b_x96: U256,
    liquidity: u128,
    round_up: bool,
) -> Result<U256, ArbRsError> {
    if sqrt_ratio_a_x96 > sqrt_ratio_b_x96 {
        std::mem::swap(&mut sqrt_ratio_a_x96, &mut sqrt_ratio_b_x96);
    }
    let numerator_1 = U256::from(liquidity) << 96;
    let numerator_2 = sqrt_ratio_b_x96 - sqrt_ratio_a_x96;

    if sqrt_ratio_a_x96.is_zero() {
        return Err(ArbRsError::UniswapV3MathError("R".into()));
    }

    let result = if round_up {
        div_rounding_up(
            mul_div_rounding_up(numerator_1, numerator_2, sqrt_ratio_b_x96).ok_or(ArbRsError::UniswapV3MathError("mul_div failed".into()))?,
            sqrt_ratio_a_x96,
        )
    } else {
        mul_div(numerator_1, numerator_2, sqrt_ratio_b_x96).ok_or(ArbRsError::UniswapV3MathError("mul_div failed".into()))? / sqrt_ratio_a_x96
    };
    Ok(result)
}

pub fn get_amount1_delta(
    mut sqrt_ratio_a_x96: U256,
    mut sqrt_ratio_b_x96: U256,
    liquidity: u128,
    round_up: bool,
) -> Result<U256, ArbRsError> {
    if sqrt_ratio_a_x96 > sqrt_ratio_b_x96 {
        std::mem::swap(&mut sqrt_ratio_a_x96, &mut sqrt_ratio_b_x96);
    }

    if round_up {
        mul_div_rounding_up(U256::from(liquidity), sqrt_ratio_b_x96 - sqrt_ratio_a_x96, Q96)
            .ok_or(ArbRsError::UniswapV3MathError("mul_div_rounding_up failed".into()))
    } else {
        mul_div(U256::from(liquidity), sqrt_ratio_b_x96 - sqrt_ratio_a_x96, Q96)
            .ok_or(ArbRsError::UniswapV3MathError("mul_div failed".into()))
    }
}

pub fn get_amount0_delta_signed(
    sqrt_ratio_a_x96: U256,
    sqrt_ratio_b_x96: U256,
    liquidity: i128,
) -> Result<I256, ArbRsError> {
    if liquidity < 0 {
        Ok(-I256::from_raw(get_amount0_delta(sqrt_ratio_a_x96, sqrt_ratio_b_x96, (-liquidity) as u128, false)?))
    } else {
        Ok(I256::from_raw(get_amount0_delta(sqrt_ratio_a_x96, sqrt_ratio_b_x96, liquidity as u128, true)?))
    }
}

pub fn get_amount1_delta_signed(
    sqrt_ratio_a_x96: U256,
    sqrt_ratio_b_x96: U256,
    liquidity: i128,
) -> Result<I256, ArbRsError> {
    if liquidity < 0 {
        Ok(-I256::from_raw(get_amount1_delta(sqrt_ratio_a_x96, sqrt_ratio_b_x96, (-liquidity) as u128, false)?))
    } else {
        Ok(I256::from_raw(get_amount1_delta(sqrt_ratio_a_x96, sqrt_ratio_b_x96, liquidity as u128, true)?))
    }
}