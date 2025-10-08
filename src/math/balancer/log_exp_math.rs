use crate::{errors::ArbRsError, math::balancer::fixed_point as fp};
use alloy_primitives::{I256, U256};
use once_cell::sync::Lazy;
use std::str::FromStr;

static ONE_18: Lazy<U256> = Lazy::new(|| U256::from(1_000_000_000_000_000_000u64));
static ONE_20: Lazy<U256> = Lazy::new(|| U256::from(100_000_000_000_000_000_000u128));
static ONE_36: Lazy<U256> = Lazy::new(|| U256::from_str("1000000000000000000000000000000000000").unwrap());

static MAX_NATURAL_EXPONENT: Lazy<I256> = Lazy::new(|| I256::from_raw(U256::from(130) * *ONE_18));
static MIN_NATURAL_EXPONENT: Lazy<I256> = Lazy::new(|| (I256::from_raw(U256::from(41) * *ONE_18)).wrapping_neg());

static LN_36_LOWER_BOUND: Lazy<U256> = Lazy::new(|| *ONE_18 - U256::from(10).pow(U256::from(17)));
static LN_36_UPPER_BOUND: Lazy<U256> = Lazy::new(|| *ONE_18 + U256::from(10).pow(U256::from(17)));
static MILD_EXPONENT_BOUND: Lazy<U256> = Lazy::new(|| U256::MAX / *ONE_20);

// Pre-computed values for e^x
static X0: Lazy<U256> = Lazy::new(|| U256::from(128) * *ONE_18);
static A0: Lazy<U256> = Lazy::new(|| U256::from_str("38877084059945950922200000000000000000000000000000000000").unwrap());
static X1: Lazy<U256> = Lazy::new(|| U256::from(64) * *ONE_18);
static A1: Lazy<U256> = Lazy::new(|| U256::from_str("6235149080811616882910000000").unwrap());
static X2: Lazy<U256> = Lazy::new(|| U256::from(32) * *ONE_18);
static A2: Lazy<U256> = Lazy::new(|| U256::from_str("7896296018268069516100000000000000").unwrap());
static X3: Lazy<U256> = Lazy::new(|| U256::from(16) * *ONE_18);
static A3: Lazy<U256> = Lazy::new(|| U256::from_str("888611052050787263676000000").unwrap());
static X4: Lazy<U256> = Lazy::new(|| U256::from(8) * *ONE_18);
static A4: Lazy<U256> = Lazy::new(|| U256::from_str("298095798704172827474000").unwrap());
static X5: Lazy<U256> = Lazy::new(|| U256::from(4) * *ONE_18);
static A5: Lazy<U256> = Lazy::new(|| U256::from_str("5459815003314423907810").unwrap());
static X6: Lazy<U256> = Lazy::new(|| U256::from(2) * *ONE_18);
static A6: Lazy<U256> = Lazy::new(|| U256::from_str("738905609893065022723").unwrap());
static X7: Lazy<U256> = Lazy::new(|| *ONE_18);
static A7: Lazy<U256> = Lazy::new(|| U256::from_str("271828182845904523536").unwrap());
static X8: Lazy<U256> = Lazy::new(|| *ONE_18 / U256::from(2));
static A8: Lazy<U256> = Lazy::new(|| U256::from_str("164872127070012814685").unwrap());
static X9: Lazy<U256> = Lazy::new(|| *ONE_18 / U256::from(4));
static A9: Lazy<U256> = Lazy::new(|| U256::from_str("128402541668774148407").unwrap());
static X10: Lazy<U256> = Lazy::new(|| *ONE_18 / U256::from(8));
static A10: Lazy<U256> = Lazy::new(|| U256::from_str("113314845306682631683").unwrap());
static X11: Lazy<U256> = Lazy::new(|| *ONE_18 / U256::from(16));
static A11: Lazy<U256> = Lazy::new(|| U256::from_str("106449445891785942956").unwrap());


/// Calculates x^y in fixed point math using x^y = e^(y * ln(x))
pub fn pow(x: U256, y: U256) -> Result<U256, ArbRsError> {
    if y.is_zero() { return Ok(*ONE_18); }
    if x.is_zero() { return Ok(U256::ZERO); }
    if y >= *MILD_EXPONENT_BOUND { return Err(ArbRsError::CalculationError("pow y out of bounds".into())); }

    let logx_times_y = {
        let y_i256 = I256::try_from(y).map_err(|_| ArbRsError::CalculationError("y does not fit in I256".into()))?;
        if x > *LN_36_LOWER_BOUND && x < *LN_36_UPPER_BOUND {
            let ln_36_x = _ln_36(x)?;
            let term1 = (ln_36_x / *ONE_18) * y;
            let term2 = ((ln_36_x % *ONE_18) * y) / *ONE_18;
            I256::try_from(term1 + term2).map_err(|_| ArbRsError::CalculationError("logx*y does not fit in I256".into()))?
        } else {
            _ln(x)?.wrapping_mul(y_i256)
        }
    };

    let logx_times_y = logx_times_y / I256::from_raw(*ONE_18);

    if logx_times_y < *MIN_NATURAL_EXPONENT || logx_times_y > *MAX_NATURAL_EXPONENT {
        return Err(ArbRsError::CalculationError("product out of bounds".into()));
    }

    exp(logx_times_y)
}

/// Calculates the natural exponent e^x for a fixed-point x.
pub fn exp(x: I256) -> Result<U256, ArbRsError> {
    if x < *MIN_NATURAL_EXPONENT || x > *MAX_NATURAL_EXPONENT {
        return Err(ArbRsError::CalculationError("Invalid exponent".into()));
    }
    if x.is_negative() {
        let denominator = exp(x.wrapping_neg())?;
        // e^(-x) = 1 / e^x
        return fp::div_down(*ONE_18, denominator);
    }
    
    let mut x_u256 = x.into_raw();
    let first_an = if x_u256 >= *X0 { x_u256 -= *X0; *A0 }
                   else if x_u256 >= *X1 { x_u256 -= *X1; *A1 }
                   else { U256::from(1) };

    let mut x_rem = x_u256 * U256::from(100);
    let mut product = *ONE_20;

    if x_rem >= *X2 { x_rem -= *X2; product = (product * *A2) / *ONE_20; }
    if x_rem >= *X3 { x_rem -= *X3; product = (product * *A3) / *ONE_20; }
    if x_rem >= *X4 { x_rem -= *X4; product = (product * *A4) / *ONE_20; }
    if x_rem >= *X5 { x_rem -= *X5; product = (product * *A5) / *ONE_20; }
    if x_rem >= *X6 { x_rem -= *X6; product = (product * *A6) / *ONE_20; }
    if x_rem >= *X7 { x_rem -= *X7; product = (product * *A7) / *ONE_20; }
    if x_rem >= *X8 { x_rem -= *X8; product = (product * *A8) / *ONE_20; }
    if x_rem >= *X9 { x_rem -= *X9; product = (product * *A9) / *ONE_20; }

    let mut series_sum = *ONE_20;
    let mut term = x_rem;
    series_sum += term;

    term = (term * x_rem) / *ONE_20 / U256::from(2); series_sum += term;
    term = (term * x_rem) / *ONE_20 / U256::from(3); series_sum += term;
    term = (term * x_rem) / *ONE_20 / U256::from(4); series_sum += term;
    term = (term * x_rem) / *ONE_20 / U256::from(5); series_sum += term;
    term = (term * x_rem) / *ONE_20 / U256::from(6); series_sum += term;
    term = (term * x_rem) / *ONE_20 / U256::from(7); series_sum += term;
    
    Ok((((product * series_sum) / *ONE_20) * first_an) / U256::from(100))
}

/// Calculates the natural logarithm ln(a) for a fixed-point a.
fn _ln(a: U256) -> Result<I256, ArbRsError> {
    if a < *ONE_18 {
        let inverted_a = fp::div_down(*ONE_18, a)?;
        return Ok(_ln(inverted_a)?.wrapping_neg());
    }
    let mut a_rem = a;
    let mut sum = I256::ZERO;

    if a_rem >= *A0 * *ONE_18 { a_rem /= *A0; sum += I256::from_raw(*X0); }
    if a_rem >= *A1 * *ONE_18 { a_rem /= *A1; sum += I256::from_raw(*X1); }
    
    let mut sum = sum * I256::from_raw(U256::from(100u64));
    a_rem *= U256::from(100);

    if a_rem >= *A2 { a_rem = (a_rem * *ONE_20) / *A2; sum += I256::from_raw(*X2); }
    if a_rem >= *A3 { a_rem = (a_rem * *ONE_20) / *A3; sum += I256::from_raw(*X3); }
    if a_rem >= *A4 { a_rem = (a_rem * *ONE_20) / *A4; sum += I256::from_raw(*X4); }
    if a_rem >= *A5 { a_rem = (a_rem * *ONE_20) / *A5; sum += I256::from_raw(*X5); }
    if a_rem >= *A6 { a_rem = (a_rem * *ONE_20) / *A6; sum += I256::from_raw(*X6); }
    if a_rem >= *A7 { a_rem = (a_rem * *ONE_20) / *A7; sum += I256::from_raw(*X7); }
    if a_rem >= *A8 { a_rem = (a_rem * *ONE_20) / *A8; sum += I256::from_raw(*X8); }
    if a_rem >= *A9 { a_rem = (a_rem * *ONE_20) / *A9; sum += I256::from_raw(*X9); }
    if a_rem >= *A10 { a_rem = (a_rem * *ONE_20) / *A10; sum += I256::from_raw(*X10); }
    if a_rem >= *A11 { a_rem = (a_rem * *ONE_20) / *A11; sum += I256::from_raw(*X11); }

    let z = fp::div_down(a_rem - *ONE_20, a_rem + *ONE_20)?;
    let z_squared = fp::mul_down(z, z)?;
    
    let mut num = z;
    let mut series_sum = z;
    
    num = fp::mul_down(num, z_squared)?; series_sum += fp::div_down(num, U256::from(3))?;
    num = fp::mul_down(num, z_squared)?; series_sum += fp::div_down(num, U256::from(5))?;
    num = fp::mul_down(num, z_squared)?; series_sum += fp::div_down(num, U256::from(7))?;
    num = fp::mul_down(num, z_squared)?; series_sum += fp::div_down(num, U256::from(9))?;
    num = fp::mul_down(num, z_squared)?; series_sum += fp::div_down(num, U256::from(11))?;

    let series_sum_i256 = I256::try_from(series_sum * U256::from(2)).map_err(|_| ArbRsError::CalculationError("series_sum does not fit".into()))?;

    Ok((sum + series_sum_i256) / I256::from_raw(U256::from(100u64)))
}

fn _ln_36(x: U256) -> Result<U256, ArbRsError> {
    let x = x * *ONE_18;

    let z = fp::div_down(x - *ONE_36, x + *ONE_36)?;
    let z_squared = fp::mul_down(z, z)?;

    let mut num = z;
    let mut series_sum = z;

    num = fp::mul_down(num, z_squared)?; series_sum += fp::div_down(num, U256::from(3))?;
    num = fp::mul_down(num, z_squared)?; series_sum += fp::div_down(num, U256::from(5))?;
    num = fp::mul_down(num, z_squared)?; series_sum += fp::div_down(num, U256::from(7))?;
    num = fp::mul_down(num, z_squared)?; series_sum += fp::div_down(num, U256::from(9))?;
    num = fp::mul_down(num, z_squared)?; series_sum += fp::div_down(num, U256::from(11))?;
    num = fp::mul_down(num, z_squared)?; series_sum += fp::div_down(num, U256::from(13))?;
    num = fp::mul_down(num, z_squared)?; series_sum += fp::div_down(num, U256::from(15))?;

    Ok(series_sum * U256::from(2))
}