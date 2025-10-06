use crate::{
    arbitrage::types::Arbitrage,
    errors::ArbRsError,
    pool::PoolSnapshot,
};
use alloy_primitives::{Address, U256};
use alloy_provider::Provider;
use std::{collections::HashMap, sync::Arc};

const INV_PHI_SCALED: U256 = U256::from_limbs([618_034, 0, 0, 0]);
const SCALE: U256 = U256::from_limbs([1_000_000, 0, 0, 0]);

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