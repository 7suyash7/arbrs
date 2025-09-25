use alloy_primitives::{Address, U256, address};

// Math Constants
pub const PRECISION: U256 = U256::from_limbs([1_000_000_000_000_000_000, 0, 0, 0]); // 10^18
pub const A_PRECISION: U256 = U256::from_limbs([100, 0, 0, 0]);
pub const FEE_DENOMINATOR: U256 = U256::from_limbs([10_000_000_000, 0, 0, 0]); // 10^10

// Well-Known Pool Addresses
pub const TRIPOOL_ADDRESS: Address = address!("bEbc44782C7dB0a1A60Cb6fe97d0b483032FF1C7");
pub const RAI3CRV_METAPOOL_ADDRESS: Address = address!("618788357D0EBd8A37e763ADab3bc575D54c2C7d");
pub const COMPOUND_POOL_ADDRESS: Address = address!("A2B47E3D5c44877cca798226B7B8118F9BFb7A56");

// Broken Pools
pub const BROKEN_POOLS: &[Address] = &[
    address!("110cc323ca53d622469EdD217387E2E6B33F1dF5"),
    address!("1F71f05CF491595652378Fe94B7820344A551B8E"),
    address!("28B0Cf1baFB707F2c6826d10caf6DD901a6540C5"),
    address!("3685646651FCcC80e7CCE7Ee24c5f47Ed9b434ac"),
    address!("84997FAFC913f1613F51Bb0E2b5854222900514B"),
    address!("9CA41a2DaB3CEE15308998868ca644e2e3be5C59"),
    address!("A77d09743F77052950C4eb4e6547E9665299BecD"),
    address!("D652c40fBb3f06d6B58Cb9aa9CFF063eE63d465D"),
];
