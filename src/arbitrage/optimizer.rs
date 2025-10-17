use crate::{arbitrage::types::Arbitrage, errors::ArbRsError, pool::PoolSnapshot};
use alloy_primitives::{Address, U256};
use alloy_provider::Provider;
use std::{collections::HashMap, sync::Arc};

const INV_PHI_SCALED: U256 = U256::from_limbs([618_034, 0, 0, 0]);
const SCALE: U256 = U256::from_limbs([1_000_000, 0, 0, 0]);
pub const FLASHLOAN_FEE_BPS: U256 = U256::from_limbs([9, 0, 0, 0]);
pub const BPS_DENOMINATOR: U256 = U256::from_limbs([10_000, 0, 0, 0]);
pub const ESTIMATED_GAS_UNITS: U256 = U256::from_limbs([700_000, 0, 0, 0]); 
pub const ETHER_SCALE: U256 = U256::from_limbs([1_000_000_000_000_000_000, 0, 0, 0]);
pub const MIN_NET_PROFIT_THRESHOLD: U256 = U256::from_limbs([50_000_000_000_000_000, 0, 0, 0]);

/// Finds the optimal input amount for a given arbitrage path using Golden-section search.
pub fn find_optimal_input<P>(
    path: &Arc<dyn Arbitrage<P>>,
    mut a: U256,
    mut b: U256,
    snapshots: &HashMap<Address, PoolSnapshot>,
) -> Result<(U256, U256), ArbRsError>
where
    P: Provider + Send + Sync + 'static + ?Sized,
{
    let tolerance = U256::from(10).pow(U256::from(15));

    let mut c = b - (b - a) * INV_PHI_SCALED / SCALE;
    let mut d = a + (b - a) * INV_PHI_SCALED / SCALE;

    while (b - a) > tolerance {
        let profit_c = path.calculate_out_amount(c, snapshots)?.saturating_sub(c);
        let profit_d = path.calculate_out_amount(d, snapshots)?.saturating_sub(d);

        if profit_c > profit_d {
            b = d;
        } else {
            a = c;
        }

        c = b - (b - a) * INV_PHI_SCALED / SCALE;
        d = a + (b - a) * INV_PHI_SCALED / SCALE;
    }

    let optimal_input = (a + b) / U256::from(2);
    let max_profit = path
        .calculate_out_amount(optimal_input, snapshots)?
        .saturating_sub(optimal_input);

    Ok((optimal_input, max_profit))
}

pub fn find_max_capacity<P>(
    path: &Arc<dyn Arbitrage<P>>,
    mut a: U256,
    mut b: U256,
    snapshots: &HashMap<Address, PoolSnapshot>,
    min_net_profit: U256,
    gas_cost_in_profit_token: U256,
) -> Result<U256, ArbRsError>
where
    P: Provider + Send + Sync + 'static + ?Sized,
{
    let calculate_net_profit = |x: U256| -> Result<U256, ArbRsError> {
        if x.is_zero() { return Ok(U256::ZERO); }

        let gross_out = path.calculate_out_amount(x, snapshots)?;
        let gross_profit = gross_out.saturating_sub(x);

        let flashloan_fee = x
            .checked_mul(FLASHLOAN_FEE_BPS)
            .unwrap_or_default()
            .checked_div(BPS_DENOMINATOR)
            .unwrap_or_default();
            
        let total_cost = gas_cost_in_profit_token.saturating_add(flashloan_fee);
        
        Ok(gross_profit.saturating_sub(total_cost))
    };
    if calculate_net_profit(b)? < min_net_profit {
        let gross_a = path.calculate_out_amount(a, snapshots)?.saturating_sub(a);
        if gross_a.saturating_sub(calculate_net_profit(a)?) < min_net_profit {
             return Ok(U256::ZERO);
        }
    }

    let tolerance = U256::from_limbs([10_000_000_000_000_000, 0, 0, 0]);

    let mut high = b;
    let mut low = a;
    let mut max_capacity = U256::ZERO;

    for _ in 0..128 {
        if high.saturating_sub(low) <= tolerance {
            break;
        }

        let mid = (high.saturating_add(low)) / U256::from(2);
        if mid.is_zero() { break; }

        let net_profit_mid = calculate_net_profit(mid)?;

        if net_profit_mid >= min_net_profit {
            max_capacity = mid; 
            low = mid;
        } else {
            high = mid;
        }
    }

    Ok(max_capacity) 
}
