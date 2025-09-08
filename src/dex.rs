use alloy_primitives::{Address, address};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DexVariant {
    UniswapV2,
    SushiSwap,
    PancakeSwapV2,
}

#[derive(Debug, Clone)]
pub struct DexDetails {
    pub dex_type: DexVariant,
}

/// Creates a map of factory addresses to DEX details for mainnet (chain ID 1).
pub fn build_mainnet_dex_registry() -> HashMap<Address, DexDetails> {
    let mut registry = HashMap::new();

    // Uniswap V2 Factory
    registry.insert(
        address!("5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f"),
        DexDetails {
            dex_type: DexVariant::UniswapV2,
        },
    );

    // Sushiswap Factory
    registry.insert(
        address!("C0AEe478e3658e2610c5F7A4A2E1777cE9e4f2Ac"),
        DexDetails {
            dex_type: DexVariant::SushiSwap,
        },
    );

    // Can add more forks here later I mean i WILL add more forks here pursuing uniswapv3 rn...

    registry
}
