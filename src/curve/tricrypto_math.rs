use crate::errors::ArbRsError;
use alloy_primitives::U256;

pub const TEN_POW_18: U256 = U256::from_limbs([1_000_000_000_000_000_000, 0, 0, 0]);

/// Calculates the fee reduction coefficient based on pool imbalance.
/// Corresponds to `_reduction_coefficient` in the Python code.
pub fn reduction_coefficient(x: &[U256], fee_gamma: U256) -> Result<U256, ArbRsError> {
    let n_coins = U256::from(x.len());
    let mut k = TEN_POW_18;
    let s: U256 = x.iter().sum();

    if s.is_zero() {
        return Ok(k);
    }

    for &x_i in x {
        k = k.checked_mul(n_coins).ok_or(ArbRsError::CalculationError("k mul1 overflow".to_string()))?
            .checked_mul(x_i).ok_or(ArbRsError::CalculationError("k mul2 overflow".to_string()))?
            .checked_div(s).ok_or(ArbRsError::CalculationError("k div underflow".to_string()))?;
    }

    if fee_gamma > U256::ZERO {
        let denominator = fee_gamma + TEN_POW_18 - k;
        k = fee_gamma.checked_mul(TEN_POW_18).ok_or(ArbRsError::CalculationError("k fee_gamma mul overflow".to_string()))?
            .checked_div(denominator).ok_or(ArbRsError::CalculationError("k fee_gamma div underflow".to_string()))?;
    }

    Ok(k)
}

/// The custom Newton's method solver for the Tricrypto invariant.
/// Corresponds to `_newton_y` in the Python code.
pub fn newton_y(ann: U256, gamma: U256, xp: &[U256], d: U256, token_index: usize) -> Result<U256, ArbRsError> {
    const N_COINS: usize = 3;
    let a_multiplier = U256::from(100);

    let mut y = d / U256::from(N_COINS);
    let mut k0_i = TEN_POW_18;
    let mut s_i = U256::ZERO;

    let mut x_sorted = xp.to_vec();
    x_sorted[token_index] = U256::ZERO;
    x_sorted.sort_by(|a, b| b.cmp(a)); // Sort descending

    let convergence_limit = x_sorted[0].div_ceil(U256::from(10).pow(U256::from(14)))
        .max(d.div_ceil(U256::from(10).pow(U256::from(14))))
        .max(U256::from(100));

    for j in 2..=N_COINS {
        let _x = x_sorted[N_COINS - j];
        y = y.checked_mul(d).ok_or(ArbRsError::CalculationError("newton_y y mul overflow".to_string()))?
            .checked_div(_x.checked_mul(U256::from(N_COINS)).unwrap_or_default()).unwrap_or_default();
        s_i += _x;
    }

    for k in 0..(N_COINS - 1) {
        k0_i = k0_i.checked_mul(x_sorted[k]).ok_or(ArbRsError::CalculationError("newton_y k0i mul1 overflow".to_string()))?
            .checked_mul(U256::from(N_COINS)).ok_or(ArbRsError::CalculationError("newton_y k0i mul2 overflow".to_string()))?
            .checked_div(d).ok_or(ArbRsError::CalculationError("newton_y k0i div underflow".to_string()))?;
    }

    for _ in 0..255 {
        let y_prev = y;

        let k0 = k0_i.checked_mul(y).ok_or(ArbRsError::CalculationError("newton_y k0 mul overflow".to_string()))?
            .checked_mul(U256::from(N_COINS)).ok_or(ArbRsError::CalculationError("newton_y k0 mul2 overflow".to_string()))?
            .checked_div(d).ok_or(ArbRsError::CalculationError("newton_y k0 div underflow".to_string()))?;
        let s = s_i + y;

        let g1k0 = (gamma + TEN_POW_18).saturating_sub(k0) + U256::from(1);

        let mul1 = TEN_POW_18.checked_mul(d).ok_or(ArbRsError::CalculationError("newton_y mul1 overflow".to_string()))?
            .checked_div(gamma).ok_or(ArbRsError::CalculationError("newton_y mul1 div1 underflow".to_string()))?
            .checked_mul(g1k0).ok_or(ArbRsError::CalculationError("newton_y mul1 overflow".to_string()))?
            .checked_div(gamma).ok_or(ArbRsError::CalculationError("newton_y mul1 div2 underflow".to_string()))?
            .checked_mul(g1k0).ok_or(ArbRsError::CalculationError("newton_y mul1 overflow".to_string()))?
            .checked_mul(a_multiplier).ok_or(ArbRsError::CalculationError("newton_y mul1 overflow".to_string()))?
            .checked_div(ann).ok_or(ArbRsError::CalculationError("newton_y mul1 div3 underflow".to_string()))?;
        let mul2 = TEN_POW_18 + (U256::from(2) * TEN_POW_18).checked_mul(k0).ok_or(ArbRsError::CalculationError("newton_y mul2 overflow".to_string()))?
            .checked_div(g1k0).ok_or(ArbRsError::CalculationError("newton_y mul2 div underflow".to_string()))?;

        let yfprime = TEN_POW_18.checked_mul(y).ok_or(ArbRsError::CalculationError("newton_y yfprime overflow".to_string()))?
            + s.checked_mul(mul2).ok_or(ArbRsError::CalculationError("newton_y yfprime overflow".to_string()))?
            + mul1;
        let dyfprime = d.checked_mul(mul2).ok_or(ArbRsError::CalculationError("newton_y dyfprime overflow".to_string()))?;

        if yfprime < dyfprime {
            y = y_prev / U256::from(2);
            continue;
        }
        
        let fprime = (yfprime - dyfprime).checked_div(y).ok_or(ArbRsError::CalculationError("newton_y fprime underflow".to_string()))?;
        let mut y_minus = mul1.checked_div(fprime).unwrap_or_default();
        let y_plus = (yfprime + TEN_POW_18.checked_mul(d).unwrap_or_default()).checked_div(fprime).unwrap_or_default()
            + y_minus.checked_mul(TEN_POW_18).unwrap_or_default().checked_div(k0).unwrap_or_default();
        y_minus += TEN_POW_18.checked_mul(s).unwrap_or_default().checked_div(fprime).unwrap_or_default();
        
        y = if y_plus < y_minus { y_prev / U256::from(2) } else { y_plus - y_minus };

        let diff = if y > y_prev { y - y_prev } else { y_prev - y };
        if diff < convergence_limit.max(y / U256::from(10).pow(U256::from(14))) {
            return Ok(y);
        }
    }

    Err(ArbRsError::CalculationError("Tricrypto newton_y did not converge".to_string()))
}