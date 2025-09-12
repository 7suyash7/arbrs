use super::{full_math, sqrt_price_math};
use alloy_primitives::U256;

#[derive(Debug, PartialEq, Eq)]
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
    amount_remaining: U256,
    fee_pips: u32,
) -> Option<SwapStep> {
    let zero_for_one = sqrt_ratio_current_x96 >= sqrt_ratio_target_x96;
    let exact_in = amount_remaining > U256::ZERO;

    if exact_in {
        let amount_remaining_less_fee = full_math::mul_div(
            amount_remaining,
            U256::from(1_000_000 - fee_pips),
            U256::from(1_000_000),
        )?;

        let amount_in = if zero_for_one {
            sqrt_price_math::get_amount0_delta(
                sqrt_ratio_target_x96,
                sqrt_ratio_current_x96,
                liquidity,
                true,
            )?
        } else {
            sqrt_price_math::get_amount1_delta(
                sqrt_ratio_current_x96,
                sqrt_ratio_target_x96,
                liquidity,
                true,
            )?
        };

        let sqrt_ratio_next_x96 = if amount_remaining_less_fee >= amount_in {
            sqrt_ratio_target_x96
        } else {
            sqrt_price_math::get_next_sqrt_price_from_input(
                sqrt_ratio_current_x96,
                liquidity,
                amount_remaining_less_fee,
                zero_for_one,
            )?
        };

        let max = sqrt_ratio_target_x96 == sqrt_ratio_next_x96;

        let amount_in = if max && zero_for_one {
            amount_in
        } else if max && !zero_for_one {
            amount_in
        } else if zero_for_one {
            sqrt_price_math::get_amount0_delta(
                sqrt_ratio_next_x96,
                sqrt_ratio_current_x96,
                liquidity,
                true,
            )?
        } else {
            sqrt_price_math::get_amount1_delta(
                sqrt_ratio_current_x96,
                sqrt_ratio_next_x96,
                liquidity,
                true,
            )?
        };

        if !max {
            if amount_in > amount_remaining_less_fee {
                return None;
            }
        }

        let amount_out = if zero_for_one {
            sqrt_price_math::get_amount1_delta(
                sqrt_ratio_next_x96,
                sqrt_ratio_current_x96,
                liquidity,
                false,
            )?
        } else {
            sqrt_price_math::get_amount0_delta(
                sqrt_ratio_current_x96,
                sqrt_ratio_next_x96,
                liquidity,
                false,
            )?
        };

        let fee_amount = full_math::mul_div_rounding_up(
            amount_in,
            U256::from(fee_pips),
            U256::from(1_000_000 - fee_pips),
        )?;

        Some(SwapStep {
            sqrt_ratio_next_x96,
            amount_in: amount_in + fee_amount,
            amount_out,
            fee_amount,
        })
    } else {
        let amount_out = if zero_for_one {
            sqrt_price_math::get_amount1_delta(
                sqrt_ratio_target_x96,
                sqrt_ratio_current_x96,
                liquidity,
                false,
            )?
        } else {
            sqrt_price_math::get_amount0_delta(
                sqrt_ratio_current_x96,
                sqrt_ratio_target_x96,
                liquidity,
                false,
            )?
        };

        let sqrt_ratio_next_x96 = if amount_remaining >= amount_out {
            sqrt_ratio_target_x96
        } else {
            sqrt_price_math::get_next_sqrt_price_from_output(
                sqrt_ratio_current_x96,
                liquidity,
                amount_remaining,
                zero_for_one,
            )?
        };

        let max = sqrt_ratio_target_x96 == sqrt_ratio_next_x96;

        let amount_in = if max && zero_for_one {
            sqrt_price_math::get_amount0_delta(
                sqrt_ratio_target_x96,
                sqrt_ratio_current_x96,
                liquidity,
                true,
            )?
        } else if max && !zero_for_one {
            sqrt_price_math::get_amount1_delta(
                sqrt_ratio_current_x96,
                sqrt_ratio_target_x96,
                liquidity,
                true,
            )?
        } else if zero_for_one {
            sqrt_price_math::get_amount0_delta(
                sqrt_ratio_next_x96,
                sqrt_ratio_current_x96,
                liquidity,
                true,
            )?
        } else {
            sqrt_price_math::get_amount1_delta(
                sqrt_ratio_current_x96,
                sqrt_ratio_next_x96,
                liquidity,
                true,
            )?
        };

        let amount_out = if max && zero_for_one {
            amount_out
        } else if max && !zero_for_one {
            amount_out
        } else if zero_for_one {
            sqrt_price_math::get_amount1_delta(
                sqrt_ratio_next_x96,
                sqrt_ratio_current_x96,
                liquidity,
                false,
            )?
        } else {
            sqrt_price_math::get_amount0_delta(
                sqrt_ratio_current_x96,
                sqrt_ratio_next_x96,
                liquidity,
                false,
            )?
        };

        let fee_amount =
            full_math::mul_div_rounding_up(amount_in, U256::from(fee_pips), U256::from(1_000_000))?;

        Some(SwapStep {
            sqrt_ratio_next_x96,
            amount_in: amount_in + fee_amount,
            amount_out,
            fee_amount,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::U256;
    use crate::math::v3::utils::sqrt;
    use std::str::FromStr;
    use crate::math::v3::sqrt_price_math;

    fn e18(n: u64) -> U256 {
        U256::from(n) * U256::from(10).pow(U256::from(18))
    }

    fn encode_price_sqrt(reserve1: u128, reserve0: u128) -> U256 {
        let r1 = U256::from(reserve1);
        let r0 = U256::from(reserve0);
        sqrt(r1 * (U256::from(1) << 192) / r0)
    }

    #[test]
    fn test_swap_step_exact_in_capped_at_target_one_for_zero() {
        let price = encode_price_sqrt(1, 1);
        let price_target = encode_price_sqrt(101, 100);
        let liquidity = e18(2).to::<u128>();
        let amount = e18(1);
        let fee = 600; // 0.06%

        let result = compute_swap_step(price, price_target, liquidity, amount, fee).unwrap();

        assert_eq!(result.amount_in, U256::from_str("9975124224178055").unwrap());
        assert_eq!(result.fee_amount, U256::from_str("5988667735148").unwrap());
        assert_eq!(result.amount_out, U256::from_str("9925619580021728").unwrap());
        assert!(result.amount_in + result.fee_amount < amount);
        assert_eq!(result.sqrt_ratio_next_x96, price_target);
    }

    #[test]
    fn test_swap_step_exact_in_fully_spent_one_for_zero() {
        let price = encode_price_sqrt(1, 1);
        let price_target = encode_price_sqrt(1000, 100); // price moves a lot
        let liquidity = e18(2).to::<u128>();
        let amount = e18(1);
        let fee = 600;

        let result = compute_swap_step(price, price_target, liquidity, amount, fee).unwrap();

        assert_eq!(result.amount_in, U256::from_str("999400000000000000").unwrap());
        assert_eq!(result.fee_amount, U256::from(600000000000000_u128));
        assert_eq!(result.amount_out, U256::from_str("666399946655997866").unwrap());
        assert_eq!(result.amount_in + result.fee_amount, amount);
        
        let price_after = sqrt_price_math::get_next_sqrt_price_from_input(
            price,
            liquidity,
            amount - result.fee_amount,
            false
        ).unwrap();
        
        assert!(result.sqrt_ratio_next_x96 < price_target);
        assert_eq!(result.sqrt_ratio_next_x96, price_after);
    }
}
