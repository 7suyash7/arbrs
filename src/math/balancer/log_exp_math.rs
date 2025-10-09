use crate::errors::ArbRsError;
use num_bigint::BigInt;
use num_traits::{One, Signed, Zero};
use once_cell::sync::Lazy;
use std::str::FromStr;

static ONE_18: Lazy<BigInt> = Lazy::new(|| BigInt::from(10).pow(18));
static ONE_20: Lazy<BigInt> = Lazy::new(|| BigInt::from(10).pow(20));
static ONE_36: Lazy<BigInt> = Lazy::new(|| BigInt::from(10).pow(36));
static MAX_NATURAL_EXPONENT: Lazy<BigInt> = Lazy::new(|| BigInt::from(130) * &*ONE_18);
static MIN_NATURAL_EXPONENT: Lazy<BigInt> = Lazy::new(|| BigInt::from(-41) * &*ONE_18);
static LN_36_LOWER_BOUND: Lazy<BigInt> = Lazy::new(|| &*ONE_18 - BigInt::from(10).pow(17));
static LN_36_UPPER_BOUND: Lazy<BigInt> = Lazy::new(|| &*ONE_18 + BigInt::from(10).pow(17));
static MILD_EXPONENT_BOUND: Lazy<BigInt> =
    Lazy::new(|| (BigInt::from(1) << 254) / &*ONE_20);

static X0: Lazy<BigInt> = Lazy::new(|| BigInt::from(128) * &*ONE_18);
static A0: Lazy<BigInt> = Lazy::new(|| BigInt::from_str("38877084059945950922200000000000000000000000000000000000").unwrap());
static X1: Lazy<BigInt> = Lazy::new(|| BigInt::from(64) * &*ONE_18);
static A1: Lazy<BigInt> = Lazy::new(|| BigInt::from_str("6235149080811616882910000000").unwrap());
static X2: Lazy<BigInt> = Lazy::new(|| BigInt::from(32) * &*ONE_18);
static A2: Lazy<BigInt> = Lazy::new(|| BigInt::from_str("7896296018268069516100000000000000").unwrap());
static X3: Lazy<BigInt> = Lazy::new(|| BigInt::from(16) * &*ONE_18);
static A3: Lazy<BigInt> = Lazy::new(|| BigInt::from_str("888611052050787263676000000").unwrap());
static X4: Lazy<BigInt> = Lazy::new(|| BigInt::from(8) * &*ONE_18);
static A4: Lazy<BigInt> = Lazy::new(|| BigInt::from_str("298095798704172827474000").unwrap());
static X5: Lazy<BigInt> = Lazy::new(|| BigInt::from(4) * &*ONE_18);
static A5: Lazy<BigInt> = Lazy::new(|| BigInt::from_str("5459815003314423907810").unwrap());
static X6: Lazy<BigInt> = Lazy::new(|| BigInt::from(2) * &*ONE_18);
static A6: Lazy<BigInt> = Lazy::new(|| BigInt::from_str("738905609893065022723").unwrap());
static X7: Lazy<BigInt> = Lazy::new(|| ONE_18.clone());
static A7: Lazy<BigInt> = Lazy::new(|| BigInt::from_str("271828182845904523536").unwrap());
static X8: Lazy<BigInt> = Lazy::new(|| &*ONE_18 / BigInt::from(2));
static A8: Lazy<BigInt> = Lazy::new(|| BigInt::from_str("164872127070012814685").unwrap());
static X9: Lazy<BigInt> = Lazy::new(|| &*ONE_18 / BigInt::from(4));
static A9: Lazy<BigInt> = Lazy::new(|| BigInt::from_str("128402541668774148407").unwrap());
static X10: Lazy<BigInt> = Lazy::new(|| &*ONE_18 / BigInt::from(8));
static A10: Lazy<BigInt> = Lazy::new(|| BigInt::from_str("113314845306682631683").unwrap());
static X11: Lazy<BigInt> = Lazy::new(|| &*ONE_18 / BigInt::from(16));
static A11: Lazy<BigInt> = Lazy::new(|| BigInt::from_str("106449445891785942956").unwrap());

pub fn pow(x: &BigInt, y: &BigInt) -> Result<BigInt, ArbRsError> {
    println!("\n--- pow(x, y) ---");
    println!("x: {}", x);
    println!("y: {}", y);
    if y.is_zero() { return Ok(ONE_18.clone()); }
    if x.is_zero() { return Ok(BigInt::zero()); }
    if x >= &(BigInt::from(1) << 255) { return Err(ArbRsError::CalculationError("X_OUT_OF_BOUNDS".into())); }
    if y >= &*MILD_EXPONENT_BOUND { return Err(ArbRsError::CalculationError("Y_OUT_OF_BOUNDS".into())); }

    let logx_times_y = if x > &*LN_36_LOWER_BOUND && x < &*LN_36_UPPER_BOUND {
        let ln_36_x = _ln_36(x)?;
        println!("Using ln_36 path. ln_36(x): {}", ln_36_x);
        (ln_36_x.clone() / &*ONE_18) * y + ((ln_36_x % &*ONE_18) * y) / &*ONE_18
    } else {
        let ln_x = _ln(x)?;
        println!("Using ln path. ln(x): {}", ln_x);
        ln_x * y
    };
    
    let final_product = &logx_times_y / &*ONE_18;
    println!("y * ln(x) (scaled e18):      {}", final_product);
    println!("MIN_NATURAL_EXPONENT:        {}", *MIN_NATURAL_EXPONENT);
    println!("MAX_NATURAL_EXPONENT:        {}", *MAX_NATURAL_EXPONENT);

    if final_product < *MIN_NATURAL_EXPONENT || final_product > *MAX_NATURAL_EXPONENT {
        println!("!!! PRODUCT OUT OF BOUNDS! !!!");
        return Err(ArbRsError::CalculationError("product out of bounds".into()));
    }
    exp(&final_product)
}

fn exp(x: &BigInt) -> Result<BigInt, ArbRsError> {
    println!("\n--- exp(x={}) ---", x);
    if x < &*MIN_NATURAL_EXPONENT || x > &*MAX_NATURAL_EXPONENT {
        return Err(ArbRsError::CalculationError("Invalid exponent".into()));
    }
    if x.is_negative() {
        println!("x is negative, returning 1/exp(-x)");
        return Ok((&*ONE_18 * &*ONE_18) / exp(&(-x))?);
    }
    let mut x = x.clone();
    let first_an = if x >= *X0 { x -= &*X0; A0.clone() }
                   else if x >= *X1 { x -= &*X1; A1.clone() }
                   else { BigInt::one() };
    println!("first_an (integer part): {}", first_an);
    x *= 100;
    let mut product = ONE_20.clone();
    if x >= *X2 { x -= &*X2; product = (&product * &*A2) / &*ONE_20; }
    if x >= *X3 { x -= &*X3; product = (&product * &*A3) / &*ONE_20; }
    if x >= *X4 { x -= &*X4; product = (&product * &*A4) / &*ONE_20; }
    if x >= *X5 { x -= &*X5; product = (&product * &*A5) / &*ONE_20; }
    if x >= *X6 { x -= &*X6; product = (&product * &*A6) / &*ONE_20; }
    if x >= *X7 { x -= &*X7; product = (&product * &*A7) / &*ONE_20; }
    if x >= *X8 { x -= &*X8; product = (&product * &*A8) / &*ONE_20; }
    if x >= *X9 { x -= &*X9; product = (&product * &*A9) / &*ONE_20; }
    let mut series_sum = ONE_20.clone();
    let mut term = x.clone();
    series_sum += &term;
    for d in 2..=12 {
        term = (&term * &x) / &*ONE_20 / d;
        series_sum += &term;
    }
    println!("Taylor series result (e20): {}", series_sum);
    let result = (((product * series_sum) / &*ONE_20) * first_an) / 100;
    println!("Final exp result (e18): {}", result);
    Ok(result)
}

fn _ln(a: &BigInt) -> Result<BigInt, ArbRsError> {
    println!("\n--- _ln(a={}) ---", a);
    if a < &*ONE_18 {
        let inverted_a = (&*ONE_18 * &*ONE_18) / a;
        println!("a < 1, inverting to {} and negating", inverted_a);
        return Ok(-_ln(&inverted_a)?);
    }
    let mut sum = BigInt::zero();
    let mut x = a.clone();
    if x >= &*A0 * &*ONE_18 { x /= &*A0; sum += &*X0; }
    if x >= &*A1 * &*ONE_18 { x /= &*A1; sum += &*X1; }
    println!("After A0/A1 -> sum (e18): {}, x (e18): {}", sum, x);
    sum *= 100;
    x *= 100;
    println!("After scaling -> sum (e20): {}, x (e20): {}", sum, x);
    if x >= *A2 { x = (&x * &*ONE_20) / &*A2; sum += &*X2; }
    if x >= *A3 { x = (&x * &*ONE_20) / &*A3; sum += &*X3; }
    if x >= *A4 { x = (&x * &*ONE_20) / &*A4; sum += &*X4; }
    if x >= *A5 { x = (&x * &*ONE_20) / &*A5; sum += &*X5; }
    if x >= *A6 { x = (&x * &*ONE_20) / &*A6; sum += &*X6; }
    if x >= *A7 { x = (&x * &*ONE_20) / &*A7; sum += &*X7; }
    if x >= *A8 { x = (&x * &*ONE_20) / &*A8; sum += &*X8; }
    if x >= *A9 { x = (&x * &*ONE_20) / &*A9; sum += &*X9; }
    if x >= *A10 { x = (&x * &*ONE_20) / &*A10; sum += &*X10; }
    if x >= *A11 { x = (&x * &*ONE_20) / &*A11; sum += &*X11; }
    println!("After A2-A11 -> sum (e20): {}, x (e20): {}", sum, x);
    let z = ((&x - &*ONE_20) * &*ONE_20) / (&x + &*ONE_20);
    let z_squared = (&z * &z) / &*ONE_20;
    println!("Taylor series -> z (e20): {}, z^2 (e20): {}", z, z_squared);
    let mut num = z.clone();
    let mut series_sum = num.clone();
    for d in [3, 5, 7, 9, 11].iter() {
        num = (&num * &z_squared) / &*ONE_20;
        series_sum += &num / d;
    }
    series_sum *= 2;
    println!("Taylor series result * 2 (e20): {}", series_sum);
    let final_sum = (sum + series_sum) / 100;
    println!("Final _ln result (e18): {}", final_sum);
    Ok(final_sum)
}

fn _ln_36(x: &BigInt) -> Result<BigInt, ArbRsError> {
    println!("\n--- _ln_36(x={}) ---", x);
    let one_36 = BigInt::from(10).pow(36);
    let x_36 = x * &*ONE_18;
    let z = ((&x_36 - &one_36) * &one_36) / (&x_36 + &one_36);
    let z_squared = (&z * &z) / &one_36;
    let mut num = z.clone();
    let mut series_sum = num.clone();
    for d in [3, 5, 7, 9, 11, 13, 15].iter() {
        num = (&num * &z_squared) / &one_36;
        series_sum += &num / d;
    }
    let result = series_sum * 2;
    println!("Final _ln_36 result (e36): {}", result);
    Ok(result)
}