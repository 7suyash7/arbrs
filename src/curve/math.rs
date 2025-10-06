use crate::curve::constants::{A_PRECISION, FEE_DENOMINATOR, PRECISION};
use crate::curve::pool_overrides::DVariant;
use crate::errors::ArbRsError;
use alloy_primitives::U256;

/// Calculates the "virtual balances" (`xp`) used in the core invariant math.
/// This normalizes token balances to a common 18-decimal precision, applying rates where necessary.
/// Formula
/// `xp_i = (balance_i * rate_i) / 10^18`
pub fn xp(rates: &[U256], balances: &[U256]) -> Result<Vec<U256>, ArbRsError> {
    if rates.len() != balances.len() {
        return Err(ArbRsError::CalculationError(
            "Rates and balances vectors must have the same length".to_string(),
        ));
    }

    let mut xp_balances = Vec::with_capacity(balances.len());

    for (rate, balance) in rates.iter().zip(balances.iter()) {
        let virtual_balance = rate
            .checked_mul(*balance)
            .ok_or_else(|| ArbRsError::CalculationError("xp mul overflow".to_string()))?
            .checked_div(PRECISION)
            .ok_or_else(|| {
                ArbRsError::CalculationError("xp div by PRECISION failed".to_string())
            })?;

        xp_balances.push(virtual_balance);
    }

    Ok(xp_balances)
}

pub(super) fn calc_dp_default(d: U256, xp: &[U256], n_coins: U256) -> Result<U256, ArbRsError> {
    let mut d_p = d;
    for &x in xp {
        if x.is_zero() {
            return Err(ArbRsError::CalculationError(
                "Cannot calculate with zero balance".to_string(),
            ));
        }
        let denominator = x.checked_mul(n_coins).ok_or(ArbRsError::CalculationError(
            "dp denominator overflow".to_string(),
        ))?;
        d_p = d_p
            .checked_mul(d)
            .ok_or(ArbRsError::CalculationError("dp mul overflow".to_string()))?
            .checked_div(denominator)
            .ok_or(ArbRsError::CalculationError("dp div underflow".to_string()))?;
    }
    Ok(d_p)
}

pub(super) fn calc_dp_alpha(d: U256, xp: &[U256], n_coins: U256) -> Result<U256, ArbRsError> {
    let mut d_p = d;
    for &x in xp {
        if x.is_zero() {
            return Err(ArbRsError::CalculationError(
                "Cannot calculate with zero balance".to_string(),
            ));
        }
        let denominator = x.checked_mul(n_coins).ok_or(ArbRsError::CalculationError(
            "dp_alpha denominator overflow".to_string(),
        ))? + U256::from(1);
        d_p = d_p
            .checked_mul(d)
            .ok_or(ArbRsError::CalculationError(
                "dp_alpha mul overflow".to_string(),
            ))?
            .checked_div(denominator)
            .ok_or(ArbRsError::CalculationError(
                "dp_alpha div underflow".to_string(),
            ))?;
    }
    Ok(d_p)
}

pub(super) fn calc_dp_beta(d: U256, xp: &[U256], n_coins: U256) -> Result<U256, ArbRsError> {
    if xp.len() < 2 || xp[0].is_zero() || xp[1].is_zero() {
        return Err(ArbRsError::CalculationError(
            "dp_beta invalid xp".to_string(),
        ));
    }
    let n_coins_sq = n_coins
        .checked_pow(U256::from(2))
        .ok_or(ArbRsError::CalculationError(
            "n_coins^2 overflow".to_string(),
        ))?;
    d.checked_mul(d)
        .ok_or(ArbRsError::CalculationError(
            "dp_beta mul1 overflow".to_string(),
        ))?
        .checked_div(xp[0])
        .ok_or(ArbRsError::CalculationError(
            "dp_beta div1 underflow".to_string(),
        ))?
        .checked_mul(d)
        .ok_or(ArbRsError::CalculationError(
            "dp_beta mul2 overflow".to_string(),
        ))?
        .checked_div(xp[1])
        .ok_or(ArbRsError::CalculationError(
            "dp_beta div2 underflow".to_string(),
        ))?
        .checked_div(n_coins_sq)
        .ok_or(ArbRsError::CalculationError(
            "dp_beta div3 underflow".to_string(),
        ))
}

pub(super) fn calc_dp_gamma(d: U256, xp: &[U256], n_coins: U256) -> Result<U256, ArbRsError> {
    if xp.len() < 2 || xp[0].is_zero() || xp[1].is_zero() {
        return Err(ArbRsError::CalculationError(
            "dp_gamma invalid xp".to_string(),
        ));
    }
    let n_coins_pow_n = n_coins
        .checked_pow(n_coins)
        .ok_or(ArbRsError::CalculationError(
            "n_coins^n_coins overflow".to_string(),
        ))?;
    d.checked_mul(d)
        .ok_or(ArbRsError::CalculationError(
            "dp_gamma mul1 overflow".to_string(),
        ))?
        .checked_div(xp[0])
        .ok_or(ArbRsError::CalculationError(
            "dp_gamma div1 underflow".to_string(),
        ))?
        .checked_mul(d)
        .ok_or(ArbRsError::CalculationError(
            "dp_gamma mul2 overflow".to_string(),
        ))?
        .checked_div(xp[1])
        .ok_or(ArbRsError::CalculationError(
            "dp_gamma div2 underflow".to_string(),
        ))?
        .checked_div(n_coins_pow_n)
        .ok_or(ArbRsError::CalculationError(
            "dp_gamma div3 underflow".to_string(),
        ))
}

pub(super) fn calc_d_default(
    ann: U256,
    s: U256,
    d: U256,
    d_p: U256,
    n_coins: U256,
) -> Result<U256, ArbRsError> {
    let num_term1 = ann
        .checked_mul(s)
        .ok_or(ArbRsError::CalculationError(
            "d_default num1 overflow".to_string(),
        ))?
        .checked_div(A_PRECISION)
        .ok_or(ArbRsError::CalculationError(
            "d_default num1 div underflow".to_string(),
        ))?;
    let numerator = (num_term1
        + d_p
            .checked_mul(n_coins)
            .ok_or(ArbRsError::CalculationError(
                "d_default num2 overflow".to_string(),
            ))?)
    .checked_mul(d)
    .ok_or(ArbRsError::CalculationError(
        "d_default numerator overflow".to_string(),
    ))?;

    let den_term1 = ann
        .saturating_sub(A_PRECISION)
        .checked_mul(d)
        .ok_or(ArbRsError::CalculationError(
            "d_default den1 overflow".to_string(),
        ))?
        .checked_div(A_PRECISION)
        .ok_or(ArbRsError::CalculationError(
            "d_default den1 div underflow".to_string(),
        ))?;
    let denominator = den_term1
        + (n_coins + U256::from(1))
            .checked_mul(d_p)
            .ok_or(ArbRsError::CalculationError(
                "d_default den2 overflow".to_string(),
            ))?;

    numerator
        .checked_div(denominator)
        .ok_or(ArbRsError::CalculationError(
            "d_default final div underflow".to_string(),
        ))
}

pub(super) fn calc_d_alpha(
    ann: U256,
    s: U256,
    d: U256,
    d_p: U256,
    n_coins: U256,
) -> Result<U256, ArbRsError> {
    let numerator = (ann.checked_mul(s).ok_or(ArbRsError::CalculationError(
        "d_alpha num1 overflow".to_string(),
    ))? + d_p
        .checked_mul(n_coins)
        .ok_or(ArbRsError::CalculationError(
            "d_alpha num2 overflow".to_string(),
        ))?)
    .checked_mul(d)
    .ok_or(ArbRsError::CalculationError(
        "d_alpha numerator overflow".to_string(),
    ))?;

    let den_term1 = (ann - U256::from(1))
        .checked_mul(d)
        .ok_or(ArbRsError::CalculationError(
            "d_alpha den1 overflow".to_string(),
        ))?;
    let denominator = den_term1
        + (n_coins + U256::from(1))
            .checked_mul(d_p)
            .ok_or(ArbRsError::CalculationError(
                "d_alpha den2 overflow".to_string(),
            ))?;

    numerator
        .checked_div(denominator)
        .ok_or(ArbRsError::CalculationError(
            "d_alpha final div underflow".to_string(),
        ))
}

/// The core iterative loop for solving the quadratic equation to find `y`.
/// This private helper is used by both `get_y` and `get_y_d`.
///
/// Formula
/// `y = (y^2 + c) / (2y + b - d)`
fn _get_y_loop(c: U256, b: U256, d: U256) -> Result<U256, ArbRsError> {
    let mut y = d;
    for _i in 0..255 {
        let y_prev = y;
        let numerator = y.pow(U256::from(2)) + c;
        let denominator = (y
            .checked_mul(U256::from(2))
            .ok_or(ArbRsError::CalculationError("y*2 overflow".to_string()))?
            + b)
            .saturating_sub(d);

        if denominator.is_zero() {
            return Err(ArbRsError::CalculationError(
                "y denominator is zero".to_string(),
            ));
        }
        y = numerator / denominator;

        if y > y_prev {
            if y - y_prev <= U256::from(1) {
                return Ok(y);
            }
        } else if y_prev - y <= U256::from(1) {
            return Ok(y);
        }
    }
    Err(ArbRsError::CalculationError(
        "y calculation did not converge".to_string(),
    ))
}

/// Solves for the Curve invariant D using Newton's method.
///
/// This function acts as a dispatcher, selecting the correct mathematical variants
/// for the `d` and `d_p` calculations based on the `d_variant` enum, which is
/// determined at pool initialization.
pub fn get_d(
    xp: &[U256],
    amp: U256,
    n_coins_usize: usize,
    d_variant: DVariant,
) -> Result<U256, ArbRsError> {
    let n_coins = U256::from(n_coins_usize);
    let s: U256 = xp.iter().sum();

    if s.is_zero() {
        return Ok(U256::ZERO);
    }

    let mut d = s;
    let ann = amp
        .checked_mul(n_coins)
        .ok_or(ArbRsError::CalculationError("ann error bruv".to_string()))?;

    for _ in 0..255 {
        let d_prev = d;

        let d_p = match d_variant {
            DVariant::Group1 | DVariant::Group3 => calc_dp_alpha(d, xp, n_coins)?,
            DVariant::Group2 => calc_dp_beta(d, xp, n_coins)?,
            DVariant::Group4 => calc_dp_gamma(d, xp, n_coins)?,
            _ => calc_dp_default(d, xp, n_coins)?,
        };

        d = match d_variant {
            DVariant::Group0 | DVariant::Group1 => calc_d_alpha(ann, s, d, d_p, n_coins)?,
            DVariant::Legacy => calc_d_default(ann, s, d, d_p, n_coins)?,
            _ => calc_d_default(ann, s, d, d_p, n_coins)?,
        };

        if d > d_prev {
            if d - d_prev <= U256::from(1) {
                return Ok(d);
            }
        } else if d_prev - d <= U256::from(1) {
            return Ok(d);
        }
    }

    Err(ArbRsError::CalculationError(
        "D calculation did not converge".to_string(),
    ))
}

/// Calculates the output balance `y` for a swap.
/// It determines the invariant `D` internally.
pub fn get_y(
    i: usize,
    j: usize,
    x: U256,
    xp: &[U256],
    amp: U256,
    n_coins: usize,
    d_variant: DVariant,
    is_y_variant_group0: bool,
    is_y_variant_group1: bool,
) -> Result<U256, ArbRsError> {
    let effective_amp = if is_y_variant_group0 {
        amp.checked_div(A_PRECISION).ok_or_else(|| {
            ArbRsError::CalculationError("effective_amp div underflow".to_string())
        })?
    } else {
        amp
    };

    let d = get_d(xp, effective_amp, n_coins, d_variant)?;
    if d.is_zero() {
        return Ok(U256::ZERO);
    }

    let n_coins_u256 = U256::from(n_coins);
    let mut s = U256::ZERO;
    let mut c = d;

    for k in 0..n_coins {
        let _x = if k == i {
            x
        } else if k != j {
            xp[k]
        } else {
            continue;
        };
        s += _x;
        if _x.is_zero() {
            return Err(ArbRsError::CalculationError(
                "Cannot calculate y with zero balance".to_string(),
            ));
        }

        let c_denominator = _x
            .checked_mul(n_coins_u256)
            .ok_or_else(|| ArbRsError::CalculationError("y c term overflow".to_string()))?;
        c = c
            .checked_mul(d)
            .ok_or_else(|| ArbRsError::CalculationError("y c mul1 overflow".to_string()))?
            .checked_div(c_denominator)
            .ok_or_else(|| ArbRsError::CalculationError("y c div1 underflow".to_string()))?;
    }

    let ann = effective_amp
        .checked_mul(n_coins_u256)
        .ok_or_else(|| ArbRsError::CalculationError("y ann overflow".to_string()))?;

    let (b, c) = if is_y_variant_group1 {
        let c_den = ann
            .checked_mul(n_coins_u256)
            .ok_or_else(|| ArbRsError::CalculationError("y c den overflow".to_string()))?;
        let c_final = c
            .checked_mul(d)
            .ok_or_else(|| ArbRsError::CalculationError("y c mul2 overflow".to_string()))?
            .checked_div(c_den)
            .ok_or_else(|| ArbRsError::CalculationError("y c div2 underflow".to_string()))?;
        let b_final = s
            .checked_add(
                d.checked_div(ann)
                    .ok_or_else(|| ArbRsError::CalculationError("y b div underflow".to_string()))?,
            )
            .ok_or_else(|| ArbRsError::CalculationError("y b add overflow".to_string()))?;
        (b_final, c_final)
    } else {
        let c_den = ann
            .checked_mul(n_coins_u256)
            .ok_or_else(|| ArbRsError::CalculationError("y c den overflow".to_string()))?;
        let c_final = c
            .checked_mul(d)
            .ok_or_else(|| ArbRsError::CalculationError("y c mul2 overflow".to_string()))?
            .checked_mul(A_PRECISION)
            .ok_or_else(|| ArbRsError::CalculationError("y c mul3 overflow".to_string()))?
            .checked_div(c_den)
            .ok_or_else(|| ArbRsError::CalculationError("y c div2 underflow".to_string()))?;
        let b_final = s
            .checked_add(
                d.checked_mul(A_PRECISION)
                    .ok_or_else(|| ArbRsError::CalculationError("y b mul overflow".to_string()))?
                    .checked_div(ann)
                    .ok_or_else(|| ArbRsError::CalculationError("y b div underflow".to_string()))?,
            )
            .ok_or_else(|| ArbRsError::CalculationError("y b add overflow".to_string()))?;
        (b_final, c_final)
    };

    _get_y_loop(c, b, d)
}

/// Calculates the balance of a single coin `y`, given a target invariant `D`.
/// Used for `calc_withdraw_one_coin`.
pub fn get_y_d(
    amp: U256,
    i: usize,
    xp: &[U256],
    d: U256,
    n_coins: usize,
    yd_variant: bool,
) -> Result<U256, ArbRsError> {
    if d.is_zero() {
        return Ok(U256::ZERO);
    }

    let n_coins_u256 = U256::from(n_coins);
    let mut s = U256::ZERO;
    let mut c = d;

    for k in 0..n_coins {
        if k == i {
            continue;
        }
        let x = xp[k];
        s += x;
        if x.is_zero() {
            return Err(ArbRsError::CalculationError(
                "Cannot calculate y_d with zero balance".to_string(),
            ));
        }
        c = c
            .checked_mul(d)
            .ok_or(ArbRsError::CalculationError(
                "y_d c mul1 overflow".to_string(),
            ))?
            .checked_div(
                x.checked_mul(n_coins_u256)
                    .ok_or(ArbRsError::CalculationError(
                        "y_d c term overflow".to_string(),
                    ))?,
            )
            .ok_or(ArbRsError::CalculationError(
                "y_d c div1 underflow".to_string(),
            ))?;
    }

    let ann = amp
        .checked_mul(n_coins_u256)
        .ok_or(ArbRsError::CalculationError("y_d ann overflow".to_string()))?;
    let (b, c) =
        if yd_variant {
            let c_final =
                c.checked_mul(d)
                    .ok_or(ArbRsError::CalculationError(
                        "y_d c mul2 overflow".to_string(),
                    ))?
                    .checked_mul(A_PRECISION)
                    .ok_or(ArbRsError::CalculationError(
                        "y_d c mul3 overflow".to_string(),
                    ))?
                    .checked_div(ann.checked_mul(n_coins_u256).ok_or(
                        ArbRsError::CalculationError("y_d c den overflow".to_string()),
                    )?)
                    .ok_or(ArbRsError::CalculationError(
                        "y_d c div2 underflow".to_string(),
                    ))?;
            let b_final = s + d
                .checked_mul(A_PRECISION)
                .ok_or(ArbRsError::CalculationError(
                    "y_d b mul overflow".to_string(),
                ))?
                .checked_div(ann)
                .ok_or(ArbRsError::CalculationError(
                    "y_d b div underflow".to_string(),
                ))?;
            (b_final, c_final)
        } else {
            let c_final =
                c.checked_mul(d)
                    .ok_or(ArbRsError::CalculationError(
                        "y_d c mul2 overflow".to_string(),
                    ))?
                    .checked_div(ann.checked_mul(n_coins_u256).ok_or(
                        ArbRsError::CalculationError("y_d c den overflow".to_string()),
                    )?)
                    .ok_or(ArbRsError::CalculationError(
                        "y_d c div2 underflow".to_string(),
                    ))?;
            let b_final = s + d.checked_div(ann).ok_or(ArbRsError::CalculationError(
                "y_d b div underflow".to_string(),
            ))?;
            (b_final, c_final)
        };

    _get_y_loop(c, b, d)
}

/// Calculates the adjusted fee rate for pools with dynamic fees.
///
/// Formula
/// `fee_gamma / (fee_gamma + (1 - K))` where `K = prod(x) / (sum(x)/N)**N`
pub fn dynamic_fee(xpi: U256, xpj: U256, fee: U256, feemul: U256) -> Result<U256, ArbRsError> {
    if feemul <= FEE_DENOMINATOR {
        return Ok(fee);
    }
    let xps2 = (xpi + xpj).pow(U256::from(2));
    if xps2.is_zero() {
        return Ok(fee);
    }

    let term1 = (feemul - FEE_DENOMINATOR)
        .checked_mul(U256::from(4))
        .ok_or_else(|| ArbRsError::CalculationError("dyn_fee term1_1 overflow".to_string()))?
        .checked_mul(xpi)
        .ok_or_else(|| ArbRsError::CalculationError("dyn_fee term1_2 overflow".to_string()))?
        .checked_mul(xpj)
        .ok_or_else(|| ArbRsError::CalculationError("dyn_fee term1_3 overflow".to_string()))?
        .checked_div(xps2)
        .ok_or_else(|| ArbRsError::CalculationError("dyn_fee term1 div underflow".to_string()))?;

    let denominator = term1 + FEE_DENOMINATOR;

    feemul
        .checked_mul(fee)
        .ok_or_else(|| ArbRsError::CalculationError("dyn_fee numerator overflow".to_string()))?
        .checked_div(denominator)
        .ok_or_else(|| ArbRsError::CalculationError("dyn_fee final div underflow".to_string()))
}
