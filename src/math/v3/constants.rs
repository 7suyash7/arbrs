use alloy_primitives::{I256, U256};

pub const MIN_TICK: i32 = -887272;
pub const MAX_TICK: i32 = -MIN_TICK;

pub const MIN_SQRT_RATIO: U256 = U256::from_limbs([4295128739, 0, 0, 0]);
pub const MAX_SQRT_RATIO: U256 =
    U256::from_limbs([6743328256752651558, 17280870778742802505, 4294805859, 0]);

pub const SQRT_10001: I256 = I256::from_raw(U256::from_limbs([11745905768312294533, 13863, 0, 0]));
pub const TICK_LOW: I256 = I256::from_raw(U256::from_limbs([
    6552757943157144234,
    184476617836266586,
    0,
    0,
]));
pub const TICK_HIGH: I256 = I256::from_raw(U256::from_limbs([
    4998474450511881007,
    15793544031827761793,
    0,
    0,
]));

pub const Q96: U256 = U256::from_limbs([0, 0, 1, 0]);
