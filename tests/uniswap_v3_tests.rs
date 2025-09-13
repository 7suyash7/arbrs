use arbrs::math::v3::{
    sqrt_price_math::{self, MAX_U160},
    swap_math::{self},
    utils::sqrt,
};
use alloy_primitives::{I256, U256};
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
fn test_get_amount_delta_tests() {
    let amount0 = sqrt_price_math::get_amount0_delta(
        encode_price_sqrt(1, 1),
        encode_price_sqrt(121, 100),
        e18(1).to::<u128>(),
        true,
    ).unwrap();
    assert_eq!(amount0, U256::from_str("90909090909090910").unwrap());

    let amount0_rounded_down = sqrt_price_math::get_amount0_delta(
        encode_price_sqrt(1, 1),
        encode_price_sqrt(121, 100),
        e18(1).to::<u128>(),
        false,
    ).unwrap();
    assert_eq!(amount0_rounded_down, amount0 - U256::from(1));

    let amount1 = sqrt_price_math::get_amount1_delta(
        encode_price_sqrt(1, 1),
        encode_price_sqrt(121, 100),
        e18(1).to::<u128>(),
        true,
    ).unwrap();
    assert_eq!(amount1, U256::from_str("100000000000000000").unwrap());

    let amount1_rounded_down = sqrt_price_math::get_amount1_delta(
        encode_price_sqrt(1, 1),
        encode_price_sqrt(121, 100),
        e18(1).to::<u128>(),
        false,
    ).unwrap();
    assert_eq!(amount1_rounded_down, amount1 - U256::from(1));
}

#[test]
fn test_get_next_sqrt_price_from_input_tests() {
    let price = encode_price_sqrt(1, 1);
    let liquidity = e18(1).to::<u128>();
    
    assert!(sqrt_price_math::get_next_sqrt_price_from_input(price, 0, U256::from(1), true).is_err());
    assert_eq!(sqrt_price_math::get_next_sqrt_price_from_input(price, liquidity, U256::ZERO, true).unwrap(), price);
    assert_eq!(sqrt_price_math::get_next_sqrt_price_from_input(price, liquidity, U256::ZERO, false).unwrap(), price);
    assert!(sqrt_price_math::get_next_sqrt_price_from_input(MAX_U160, 1024, U256::from(1024), false).is_err());

    let sqrt_q_false = sqrt_price_math::get_next_sqrt_price_from_input(
        price, liquidity, e18(1) / U256::from(10), false,
    ).unwrap();
    assert_eq!(sqrt_q_false, U256::from_str("87150978765690771352898345369").unwrap());
    
    let sqrt_q_true = sqrt_price_math::get_next_sqrt_price_from_input(
        price, liquidity, e18(1) / U256::from(10), true,
    ).unwrap();
    assert_eq!(sqrt_q_true, U256::from_str("72025602285694852357767227579").unwrap());
}

#[test]
fn test_all_swap_scenarios() {
    // exact amount IN that gets capped at price target
    let price = encode_price_sqrt(1, 1);
    let price_target = encode_price_sqrt(101, 100);
    let liquidity = e18(2).to::<u128>();
    let amount = I256::from_raw(e18(1));
    let fee = 600;

    let result = swap_math::compute_swap_step(price, price_target, liquidity, amount, fee).unwrap();
    let pre_fee_amount_in = result.amount_in - result.fee_amount;

    assert_eq!(pre_fee_amount_in, U256::from_str("9975124224178055").unwrap());
    assert_eq!(result.fee_amount, U256::from_str("5988667735148").unwrap());
    assert_eq!(result.amount_out, U256::from_str("9925619580021728").unwrap());
    assert!(result.amount_in < amount.into_raw());
    assert_eq!(result.sqrt_ratio_next_x96, price_target);

    // exact amount OUT that gets capped at price target
    let result_out = swap_math::compute_swap_step(price, price_target, liquidity, -amount, fee).unwrap();
    let pre_fee_amount_in_2 = result_out.amount_in - result_out.fee_amount;
    assert_eq!(pre_fee_amount_in_2, U256::from_str("9975124224178055").unwrap());
    assert_eq!(result_out.fee_amount, U256::from_str("5988667735148").unwrap());
    assert_eq!(result_out.amount_out, U256::from_str("9925619580021728").unwrap());
    assert!(result_out.amount_out < (-amount).into_raw());
    assert_eq!(result_out.sqrt_ratio_next_x96, price_target);
    
    // exact amount IN that is fully spent
    let price_target_full = U256::from_str("2505413383311432194396931511005").unwrap();
    let result_full = swap_math::compute_swap_step(price, price_target_full, liquidity, amount, fee).unwrap();
    let pre_fee_amount_in_3 = result_full.amount_in - result_full.fee_amount;
    assert_eq!(pre_fee_amount_in_3, U256::from_str("999400000000000000").unwrap());
    assert_eq!(result_full.fee_amount, U256::from_str("600000000000000").unwrap());
    assert_eq!(result_full.amount_out, U256::from_str("666399946655997866").unwrap());
    assert_eq!(result_full.amount_in, amount.into_raw());

    // exact amount OUT that is fully received
    let price_target_full_out = encode_price_sqrt(10000, 100);
    let amount_out_target = I256::from_raw(e18(1));
    let result_full_out = swap_math::compute_swap_step(price, price_target_full_out, liquidity, -amount_out_target, fee).unwrap();
    let pre_fee_amount_in_4 = result_full_out.amount_in - result_full_out.fee_amount;
    assert_eq!(pre_fee_amount_in_4, U256::from_str("2000000000000000000").unwrap());
    assert_eq!(result_full_out.fee_amount, U256::from_str("1200720432259356").unwrap());
    assert_eq!(result_full_out.amount_out, amount_out_target.into_raw());

    // amount OUT is capped at the desired amount out
    let result_cap_out = swap_math::compute_swap_step(
        U256::from_str("417332158212080721273783715441582").unwrap(),
        U256::from_str("1452870262520218020823638996").unwrap(),
        159344665391607089467575320103,
        I256::from_str("-1").unwrap(),
        1,
    ).unwrap();
    assert_eq!(result_cap_out.amount_in, U256::from(2)); // pre-fee 1 + fee 1
    assert_eq!(result_cap_out.amount_out, U256::from(1));
    assert_eq!(result_cap_out.fee_amount, U256::from(1));
    assert_eq!(result_cap_out.sqrt_ratio_next_x96, U256::from_str("417332158212080721273783715441581").unwrap());
    
    // entire input amount taken as fee
    let result_fee = swap_math::compute_swap_step(
        U256::from(2413),
        U256::from_str("79887613182836312").unwrap(),
        1985041575832132834610021537970,
        I256::from_str("10").unwrap(),
        1872,
    ).unwrap();
    assert_eq!(result_fee.amount_in, U256::from(10));
    assert_eq!(result_fee.fee_amount, U256::from(10));
    assert_eq!(result_fee.amount_out, U256::ZERO);
    assert_eq!(result_fee.sqrt_ratio_next_x96, U256::from(2413));
}
