use crate::errors::ArbRsError;
use crate::math::v3::{
    full_math::{mul_div, mul_div_rounding_up},
    sqrt_price_math,
};
use alloy_primitives::{I256, U256};

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct SwapStep {
    pub sqrt_ratio_next_x96: U256,
    pub amount_in: U256,
    pub amount_out: U256,
    pub fee_amount: U256,
}

pub fn compute_swap_step(
    sqrt_ratio_current_x96: U256,
    sqrt_ratio_target_x96: U256,
    liquidity: u128,
    amount_remaining: I256,
    fee_pips: u32,
) -> Result<SwapStep, ArbRsError> {
    let zero_for_one = sqrt_ratio_current_x96 >= sqrt_ratio_target_x96;
    let exact_in = amount_remaining.is_positive();

    let sqrt_ratio_next_x96: U256;
    let mut amount_in: U256;
    let mut amount_out: U256;

    if exact_in {
        let amount_remaining_less_fee = mul_div(
            amount_remaining.into_raw(),
            U256::from(1_000_000 - fee_pips),
            U256::from(1_000_000),
        ).ok_or(ArbRsError::UniswapV3MathError("mul_div failed".into()))?;

        let amount_in_to_target = if zero_for_one {
            sqrt_price_math::get_amount0_delta(sqrt_ratio_target_x96, sqrt_ratio_current_x96, liquidity, true)?
        } else {
            sqrt_price_math::get_amount1_delta(sqrt_ratio_current_x96, sqrt_ratio_target_x96, liquidity, true)?
        };

        if amount_remaining_less_fee >= amount_in_to_target {
            sqrt_ratio_next_x96 = sqrt_ratio_target_x96;
            amount_in = amount_in_to_target;
        } else {
            amount_in = amount_remaining_less_fee;
            sqrt_ratio_next_x96 = sqrt_price_math::get_next_sqrt_price_from_input(
                sqrt_ratio_current_x96,
                liquidity,
                amount_in,
                zero_for_one,
            )?;
        }
        
        amount_out = if zero_for_one {
            sqrt_price_math::get_amount1_delta(sqrt_ratio_next_x96, sqrt_ratio_current_x96, liquidity, false)?
        } else {
            sqrt_price_math::get_amount0_delta(sqrt_ratio_current_x96, sqrt_ratio_next_x96, liquidity, false)?
        };

    } else {
        let amount_out_to_target = if zero_for_one {
            sqrt_price_math::get_amount1_delta(sqrt_ratio_target_x96, sqrt_ratio_current_x96, liquidity, false)?
        } else {
            sqrt_price_math::get_amount0_delta(sqrt_ratio_current_x96, sqrt_ratio_target_x96, liquidity, false)?
        };
        
        if (-amount_remaining).into_raw() >= amount_out_to_target {
            sqrt_ratio_next_x96 = sqrt_ratio_target_x96;
            amount_out = amount_out_to_target;
        } else {
            amount_out = (-amount_remaining).into_raw();
            sqrt_ratio_next_x96 = sqrt_price_math::get_next_sqrt_price_from_output(
                sqrt_ratio_current_x96,
                liquidity,
                amount_out,
                zero_for_one,
            )?;
        }

        amount_in = if zero_for_one {
            sqrt_price_math::get_amount0_delta(sqrt_ratio_next_x96, sqrt_ratio_current_x96, liquidity, true)?
        } else {
            sqrt_price_math::get_amount1_delta(sqrt_ratio_current_x96, sqrt_ratio_next_x96, liquidity, true)?
        };
    }

    let max = sqrt_ratio_target_x96 == sqrt_ratio_next_x96;

    if zero_for_one {
        if !max || !exact_in {
            amount_in = sqrt_price_math::get_amount0_delta(sqrt_ratio_next_x96, sqrt_ratio_current_x96, liquidity, true)?;
        }
        if !max || exact_in {
            amount_out = sqrt_price_math::get_amount1_delta(sqrt_ratio_next_x96, sqrt_ratio_current_x96, liquidity, false)?;
        }
    } else {
        if !max || !exact_in {
            amount_in = sqrt_price_math::get_amount1_delta(sqrt_ratio_current_x96, sqrt_ratio_next_x96, liquidity, true)?;
        }
        if !max || exact_in {
            amount_out = sqrt_price_math::get_amount0_delta(sqrt_ratio_current_x96, sqrt_ratio_next_x96, liquidity, false)?;
        }
    }

    if !exact_in && amount_out > (-amount_remaining).into_raw() {
        amount_out = (-amount_remaining).into_raw();
    }

    let fee_amount = if exact_in && sqrt_ratio_next_x96 != sqrt_ratio_target_x96 {
        amount_remaining.into_raw() - amount_in
    } else {
        mul_div_rounding_up(
            amount_in,
            U256::from(fee_pips),
            U256::from(1_000_000 - fee_pips),
        ).ok_or(ArbRsError::UniswapV3MathError("mul_div failed".into()))?
    };

    Ok(SwapStep {
        sqrt_ratio_next_x96,
        amount_in: amount_in + fee_amount,
        amount_out,
        fee_amount,
    })
}
