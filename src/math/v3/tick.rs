use alloy_primitives::I256;
use super::constants::{MAX_TICK, MIN_TICK};

#[derive(Debug, Clone, Copy, Default)]
pub struct Tick {
    /// The gross liquidity added or removed at this tick.
    pub liquidity_gross: u128,
    /// The net liquidity change at this tick.
    /// It is the gross liquidity if the tick is a lower tick, or -gross liquidity if it's an upper tick.
    pub liquidity_net: i128,
    /// Fee growth outside of this tick for token0.
    pub fee_growth_outside_0_x128: I256,
    /// Fee growth outside of this tick for token1.
    pub fee_growth_outside_1_x128: I256,
    /// The seconds per liquidity outside of this tick.
    pub seconds_per_liquidity_outside_x128: I256,
    /// The timestamp at which the tick was last initialized.
    pub seconds_outside: u32,
    /// The tick cumulative value outside of this tick.
    pub tick_cumulative_outside: i64,
    /// A boolean indicating if the tick is initialized.
    pub initialized: bool,
}

/// Uses integer arithmetic for ceiling division
fn get_min_tick(tick_spacing: i32) -> i32 {
    (MIN_TICK + tick_spacing - 1).div_euclid(tick_spacing) * tick_spacing
}

/// Uses integer arithmetic for floor division
fn get_max_tick(tick_spacing: i32) -> i32 {
    (MAX_TICK).div_euclid(tick_spacing) * tick_spacing
}

pub fn tick_spacing_to_max_liquidity_per_tick(tick_spacing: i32) -> u128 {
    let min_tick = get_min_tick(tick_spacing);
    let max_tick = get_max_tick(tick_spacing);
    let num_ticks = ((max_tick - min_tick) / tick_spacing) as u128 + 1;
    if num_ticks == 0 { return 0; }
    u128::MAX / num_ticks
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_tick_spacing_to_max_liquidity_per_tick() {
        let tick_spacing_low = 10;
        let tick_spacing_medium = 60;
        let tick_spacing_high = 200;

        assert_eq!(
            tick_spacing_to_max_liquidity_per_tick(tick_spacing_high),
            u128::from_str("38350317471085141830651933667504588").unwrap()
        );

        assert_eq!(
            tick_spacing_to_max_liquidity_per_tick(tick_spacing_low),
            u128::from_str("1917569901783203986719870431555990").unwrap()
        );

        assert_eq!(
            tick_spacing_to_max_liquidity_per_tick(tick_spacing_medium),
            u128::from_str("11505743598341114571880798222544994").unwrap()
        );
    }
}
