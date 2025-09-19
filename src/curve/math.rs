use crate::errors::ArbRsError;
use alloy_primitives::{U256, U512};

// Constants
const A_PRECISION: U256 = U256::from_limbs([100, 0, 0, 0]);
const PRECISION: U256 = U256::from_limbs([1000000000000000000, 0, 0, 0]);
const FEE_DENOMINATOR: U256 = U256::from_limbs([10_000_000_000u64, 0, 0, 0]);


/// Solves for the Curve stableswap invariant `D` using Newton's method.
pub fn get_d(xp: &[U256], amp: U256) -> Result<U256, ArbRsError> {
    let n_coins = U256::from(xp.len());
    let s: U256 = xp.iter().sum();

    if s.is_zero() {
        return Ok(U256::ZERO);
    }

    let mut d = s;
    
    // let ann = amp.checked_mul(n_coins).ok_or(ArbRsError::CalculationError("ANN overflow".to_string()))?;
    // NEW
    let amp_scaled = amp.checked_mul(A_PRECISION).ok_or(ArbRsError::CalculationError("amp scale overflow".to_string()))?;
    let ann = amp_scaled.checked_mul(n_coins).ok_or(ArbRsError::CalculationError("ANN overflow".to_string()))?;
    
    println!("\n--- Calculating D ---");
    println!("Initial D (sum of xp): {}", d);
    println!("Amp: {}, n_coins: {}", amp, n_coins);
    println!("ANN (A*n): {}", ann);
    println!("Balances (xp): {:?}", xp);
    println!("---------------------\n");

    for i in 0..255 {
        let mut d_p = d;
        println!("\n--- D Iteration {} ---", i);
        println!("D at start of loop: {}", d);

        for (_coin_index, &x) in xp.iter().enumerate() {
            let denominator = x.checked_mul(n_coins).ok_or(ArbRsError::CalculationError("Denominator overflow".to_string()))?;
            if denominator.is_zero() {
                continue;
            }
            let d_p_512 = d_p.widening_mul(d)
                .checked_div(U512::from(denominator))
                .ok_or(ArbRsError::CalculationError("d_p 512 division failed".to_string()))?;
            
            let limbs = d_p_512.into_limbs();
            if limbs[4..].iter().any(|&limb| limb != 0) {
                return Err(ArbRsError::CalculationError("d_p overflow after division".to_string()));
            }
            d_p = U256::from_limbs([limbs[0], limbs[1], limbs[2], limbs[3]]);
        }

        let d_prev = d;

        let numerator_left = ann.checked_mul(s).ok_or(ArbRsError::CalculationError("Numerator-left overflow".to_string()))?;
        let numerator_right = d_p.checked_mul(n_coins).ok_or(ArbRsError::CalculationError("Numerator-right overflow".to_string()))?;
        let numerator_512 = U512::from(numerator_left)
            .checked_add(U512::from(numerator_right))
            .ok_or(ArbRsError::CalculationError("Numerator 512 add overflow".to_string()))?
            .checked_mul(U512::from(d))
            .ok_or(ArbRsError::CalculationError("Numerator 512 mul overflow".to_string()))?;

        let denominator_left = ann.checked_sub(U256::from(1)).ok_or(ArbRsError::CalculationError("ANN underflow".to_string()))?
                               .checked_mul(d).ok_or(ArbRsError::CalculationError("Denominator-left overflow".to_string()))?;
        let denominator_right = n_coins.checked_add(U256::from(1)).ok_or(ArbRsError::CalculationError("n_coins+1 overflow".to_string()))?
                                .checked_mul(d_p).ok_or(ArbRsError::CalculationError("Denominator-right overflow".to_string()))?;
        let denominator = denominator_left.checked_add(denominator_right).ok_or(ArbRsError::CalculationError("Denominator overflow".to_string()))?;

        if denominator.is_zero() {
            return Err(ArbRsError::CalculationError("Division by zero in D update".to_string()));
        }

        let d_512 = numerator_512.checked_div(U512::from(denominator))
            .ok_or(ArbRsError::CalculationError("D update 512 division failed".to_string()))?;
        let d_limbs = d_512.into_limbs();
        if d_limbs[4..].iter().any(|&limb| limb != 0) {
            return Err(ArbRsError::CalculationError("D overflow after division".to_string()));
        }
        d = U256::from_limbs([d_limbs[0], d_limbs[1], d_limbs[2], d_limbs[3]]);

        if d > d_prev {
            if d - d_prev <= U256::from(1) {
                return Ok(d);
            }
        } else if d_prev - d <= U256::from(1) {
            return Ok(d);
        }
    }

    Err(ArbRsError::CalculationError("D calculation did not converge".to_string()))
}

/// Calculates the balance of a coin `j` given the balances of all other coins.
pub fn get_y(i: usize, j: usize, x: U256, xp: &[U256], amp: U256) -> Result<U256, ArbRsError> {
    let n_coins = U256::from(xp.len());
    // let d = get_d(xp, amp)?;
    let d = get_d(&xp.iter().map(|&v| v).collect::<Vec<_>>(), amp)?;


    println!("\n--- Calculating Y ---");
    println!("Input token index (i): {}", i);
    println!("Output token index (j): {}", j);
    println!("New input balance (x): {}", x);
    println!("Invariant D: {}", d);


    let mut c = d;
    let mut s = U256::ZERO;

    // let ann = amp.checked_mul(n_coins).ok_or(ArbRsError::CalculationError("ANN overflow in get_y".to_string()))?;
    let amp_scaled = amp.checked_mul(A_PRECISION).ok_or(ArbRsError::CalculationError("amp scale overflow in get_y".to_string()))?;
    let ann_for_c = amp_scaled.checked_mul(n_coins).and_then(|res| res.checked_mul(n_coins))
        .ok_or(ArbRsError::CalculationError("ANN (A*n^2) overflow in get_y".to_string()))?;

    for k in 0..xp.len() {
        let current_x = if k == i {
            x
        } else if k != j {
            xp[k]
        } else {
            continue;
        };
        s = s.checked_add(current_x).ok_or(ArbRsError::CalculationError("Sum overflow in get_y".to_string()))?;
        let denominator = current_x.checked_mul(n_coins).ok_or(ArbRsError::CalculationError("Denominator overflow in get_y".to_string()))?;
        if denominator.is_zero() {
            continue;
        }
        
        let c_512 = c.widening_mul(d)
            .checked_div(U512::from(denominator))
            .ok_or(ArbRsError::CalculationError("c div failed".to_string()))?;

        let limbs = c_512.into_limbs();
        if limbs[4..].iter().any(|&limb| limb != 0) {
            return Err(ArbRsError::CalculationError("c overflow after division".to_string()));
        }
        c = U256::from_limbs([limbs[0], limbs[1], limbs[2], limbs[3]]);
    }
    println!("c after first loop: {}", c);

    let c_div_factor = ann_for_c.checked_mul(n_coins).ok_or(ArbRsError::CalculationError("c_div_factor overflow".to_string()))?;
    if c_div_factor.is_zero() {
        return Err(ArbRsError::CalculationError("Division by zero in get_y c update".to_string()));
    }
    
    let c_numerator_512 = c.widening_mul(d);

    let c_512 = c_numerator_512
        .checked_div(U512::from(c_div_factor))
        .ok_or(ArbRsError::CalculationError("c update failed".to_string()))?;
    
    let limbs = c_512.into_limbs();
    if limbs[4..].iter().any(|&limb| limb != 0) {
        println!("\n  !!!!!!!! OVERFLOW DETECTED AT FINAL C !!!!!!!");
        println!("  c_512 = {}", c_512);
        println!("  !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!\n");
        return Err(ArbRsError::CalculationError("c overflow after final division".to_string()));
    }
    c = U256::from_limbs([limbs[0], limbs[1], limbs[2], limbs[3]]);
    println!("c after final calculation: {}", c);

    // let b = s.checked_add(d.checked_div(ann).ok_or(ArbRsError::CalculationError("b div failed".to_string()))?).ok_or(ArbRsError::CalculationError("b add overflow".to_string()))?;
    let ann_for_b = amp_scaled.checked_mul(n_coins).ok_or(ArbRsError::CalculationError("ann_for_b overflow".to_string()))?;
    let b_div_factor = d.checked_mul(A_PRECISION).ok_or(ArbRsError::CalculationError("b_div_factor overflow".to_string()))?;
    let b = s.checked_add(b_div_factor.checked_div(ann_for_b).ok_or(ArbRsError::CalculationError("b div failed".to_string()))?).ok_or(ArbRsError::CalculationError("b add overflow".to_string()))?;
    println!("b coefficient: {}", b);


    let mut y = d;
    for iter in 0..255 {
        let y_prev = y;
        println!("  y loop[{}]: y_prev = {}", iter, y_prev);
        
        let y_squared_512 = y.widening_mul(y);
        let numerator_512 = y_squared_512.checked_add(U512::from(c))
            .ok_or(ArbRsError::CalculationError("y update numerator 512 overflow".to_string()))?;

        let two_y = y.checked_mul(U256::from(2)).ok_or(ArbRsError::CalculationError("2y overflow".to_string()))?;
        let denominator = two_y.checked_add(b).ok_or(ArbRsError::CalculationError("y update denominator add overflow".to_string()))?
                           .checked_sub(d).ok_or(ArbRsError::CalculationError("y update denominator sub underflow".to_string()))?;

        if denominator.is_zero() {
            return Err(ArbRsError::CalculationError("Division by zero in y update".to_string()));
        }

        let y_512 = numerator_512.checked_div(U512::from(denominator))
            .ok_or(ArbRsError::CalculationError("y update div failed".to_string()))?;

        let y_limbs = y_512.into_limbs();
        if y_limbs[4..].iter().any(|&limb| limb != 0) {
            return Err(ArbRsError::CalculationError("y overflow after division".to_string()));
        }
        y = U256::from_limbs([y_limbs[0], y_limbs[1], y_limbs[2], y_limbs[3]]);
        println!("  y loop[{}]: y_new  = {}", iter, y);

        if y > y_prev {
            if y - y_prev <= U256::from(1) {
                println!("--- Y calculation converged ---");
                return Ok(y);
            }
        } else if y_prev - y <= U256::from(1) {
            println!("--- Y calculation converged ---");
            return Ok(y);
        }
    }

    Err(ArbRsError::CalculationError("y calculation did not converge".to_string()))
}

/// Scales balances by their rate multipliers.
fn xp(rates: &[U256], balances: &[U256]) -> Result<Vec<U256>, ArbRsError> {
    rates
        .iter()
        .zip(balances.iter())
        .map(|(&rate, &balance)| {
            rate.checked_mul(balance)
                .and_then(|product| product.checked_div(PRECISION))
                .ok_or_else(|| ArbRsError::CalculationError("xp calculation failed".to_string()))
        })
        .collect()
}

/// Calculates the output amount `dy` for a given input amount `dx`.
pub fn get_dy(
    i: usize,
    j: usize,
    dx: U256,
    balances: &[U256],
    amp: U256,
    fee: U256,
    rates: &[U256],
) -> Result<U256, ArbRsError> {
    let xp = xp(rates, balances)?;
    let x = xp[i]
        .checked_add(dx.checked_mul(rates[i]).ok_or(ArbRsError::CalculationError("dx mul failed".to_string()))?
                        .checked_div(PRECISION).ok_or(ArbRsError::CalculationError("dx div failed".to_string()))?)
        .ok_or(ArbRsError::CalculationError("x overflow in get_dy".to_string()))?;

    let y = get_y(i, j, x, &xp, amp)?;

    let dy = xp[j].checked_sub(y).ok_or(ArbRsError::CalculationError("dy underflow".to_string()))?
             .checked_sub(U256::from(1)).ok_or(ArbRsError::CalculationError("dy underflow".to_string()))?;

    let fee_amount = fee.checked_mul(dy).ok_or(ArbRsError::CalculationError("fee_amount overflow".to_string()))?
                     .checked_div(FEE_DENOMINATOR).ok_or(ArbRsError::CalculationError("fee_amount div failed".to_string()))?;

    let dy_after_fee = dy.checked_sub(fee_amount).ok_or(ArbRsError::CalculationError("final dy underflow".to_string()))?;

    dy_after_fee.checked_mul(PRECISION).ok_or(ArbRsError::CalculationError("dy final mul overflow".to_string()))?
                .checked_div(rates[j]).ok_or(ArbRsError::CalculationError("dy final div failed".to_string()))
}

/// Handles the ramping of the amplification coefficient `A`.
pub fn a(
    timestamp: u64,
    initial_a: U256,
    initial_a_time: u64,
    future_a: U256,
    future_a_time: u64,
) -> Result<U256, ArbRsError> {
    if timestamp < future_a_time {
        let t0 = U256::from(initial_a_time);
        let t1 = U256::from(future_a_time);
        let time_elapsed = U256::from(timestamp)
            .checked_sub(t0)
            .ok_or_else(|| ArbRsError::CalculationError("Timestamp before initial_a_time".to_string()))?;
        let time_range = t1
            .checked_sub(t0)
            .ok_or_else(|| ArbRsError::CalculationError("future_a_time before initial_a_time".to_string()))?;

        if future_a > initial_a {
            let a_range = future_a - initial_a;
            let a_delta = a_range.checked_mul(time_elapsed).ok_or_else(|| ArbRsError::CalculationError("A delta mul overflow".to_string()))?
                               .checked_div(time_range).ok_or_else(|| ArbRsError::CalculationError("A delta div failed".to_string()))?;
            initial_a.checked_add(a_delta).ok_or_else(|| ArbRsError::CalculationError("Final A add overflow".to_string()))
        } else {
            let a_range = initial_a - future_a;
            let a_delta = a_range.checked_mul(time_elapsed).ok_or_else(|| ArbRsError::CalculationError("A delta mul overflow".to_string()))?
                               .checked_div(time_range).ok_or_else(|| ArbRsError::CalculationError("A delta div failed".to_string()))?;
            initial_a.checked_sub(a_delta).ok_or_else(|| ArbRsError::CalculationError("Final A sub underflow".to_string()))
        }
    } else {
        Ok(future_a)
    }
}
