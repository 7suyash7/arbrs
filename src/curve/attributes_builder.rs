use crate::TokenLike;
use crate::core::token::Token;
use crate::curve::pool_attributes::{
    CalculationStrategy, PoolAttributes, PoolVariant, SwapStrategyType,
};
use crate::curve::pool_overrides::{self, DVariant};
use crate::curve::registry::CurveRegistry;
use crate::errors::ArbRsError;
use crate::manager::token_manager::TokenManager;
use alloy_primitives::{Address, U256, address};
use alloy_provider::Provider;
use alloy_rpc_types::TransactionRequest;
use alloy_sol_types::{SolCall, sol};
use std::sync::Arc;

sol! {
    function offpeg_fee_multiplier() external view returns (uint256);
    function price_oracle() external view returns (uint256);
}

const COMPOUND_POOL: Address = address!("A2B47E3D5c44877cca798226B7B8118F9BFb7A56");
const TRICRYPTO2_POOL: Address = address!("80466c64868E1ab14a1Ddf27A676C3fcBE638Fe5");
const DUSD_METAPOOL: Address = address!("DcEF968d416a41Cdac0ED8702fAC8128A64241A2");
const AAVE_POOL: Address = address!("52EA46506B9CC5Ef470C5bf89f17Dc28bB35D85C");
const GUSD_METAPOOL: Address = address!("06364f10B501e868329afBc005b3492902d6C763");
const MIM_METAPOOL: Address = address!("DeBF20617708857ebe4F679508E7b7863a8A8EeE");
const LUSD_METAPOOL: Address = address!("2dded6Da1BF5DBdF597C45fcFaa3194e53EcfeAF");
const YEARN_POOL: Address = address!("79a8C46DeA5aDa233ABaFFD40F3A0A2B1e5A4F27");
const BUSD_YEARN_POOL: Address = address!("45F783CCE6B7FF23B2ab2D70e416cdb7D6055f51");
const SUSD_POOL: Address = address!("A5407eAE9Ba41422680e2e00537571bcC53efBfD");
const RAI_METAPOOL: Address = address!("59Ab5a5b5d617E478a2479B0cAD80DA7e2831492");
const T_METAPOOL: Address = address!("BfAb6FA95E0091ed66058ad493189D2cB29385E6");
const STETH_POOL: Address = address!("DC24316b9AE028F1497c275EB9192a3Ea0f67022");
const SAAVE_POOL: Address = address!("EB16Ae0052ed37f479f7fe63849198Df1765a733");

const LENDING_POOLS: &[Address] = &[
    COMPOUND_POOL,
    AAVE_POOL,
    GUSD_METAPOOL,
    YEARN_POOL,
    BUSD_YEARN_POOL,
    address!("A5407eAE9Ba41422680e2e00537571bcC53efBfD"), // sUSD
    address!("2dded6Da1BF5DBdF597C45fcFaa3194e53EcfeAF"), // LUSD/3CRV
    address!("A5407eAE9Ba41422680e2e00537571bcC53efBfD"), // sUSD
    address!("2dded6Da1BF5DBdF597C45fcFaa3194e53EcfeAF"), // LUSD/3CRV
    address!("A96A65c051bF88B4095Ee1f2451C2A9d43F53Ae2"), // aETH
    address!("F9440930043eb3997fc70e1339dBb11F341de7A8"), // rETH
];

const UNSCALED_POOLS: &[Address] = &[
    address!("04c90C198b2eFF55716079bc06d7CCc4aa4d7512"),
    address!("320B564Fb9CF36933eC507a846ce230008631fd3"),
    address!("48fF31bBbD8Ab553Ebe7cBD84e1eA3dBa8f54957"),
    address!("55A8a39bc9694714E2874c1ce77aa1E599461E18"),
    address!("875DF0bA24ccD867f8217593ee27253280772A97"),
    address!("9D0464996170c6B9e75eED71c68B99dDEDf279e8"),
    address!("Baaa1F5DbA42C3389bDbc2c9D2dE134F5cD0Dc89"),
    address!("Da5B670CcD418a187a3066674A8002Adc9356Ad1"),
    address!("f03bD3cfE85f00bF5819AC20f0870cE8a8d1F0D8"),
    address!("FB9a265b5a1f52d97838Ec7274A0b1442efAcC87"),
];

const DYNAMIC_FEE_POOLS: &[Address] = &[STETH_POOL, SAAVE_POOL];

const ADMIN_FEE_POOLS: &[Address] = &[
    address!("4e0915C88bC70750D68C481540F081fEFaF22273"),
    address!("1005F7406f32a61BD760CfA14aCCd2737913d546"),
    address!("6A274dE3e2462c7614702474D64d376729831dCa"),
    address!("b9446c4Ef5EBE66268dA6700D26f96273DE3d571"),
    address!("3Fb78e61784C9c637D560eDE23Ad57CA1294c14a"),
];

const ORACLE_POOLS: &[Address] = &[RAI_METAPOOL, T_METAPOOL];

pub async fn build_attributes<P: Provider + Send + Sync + 'static + ?Sized>(
    address: Address,
    tokens: &[Arc<Token<P>>],
    provider: Arc<P>,
    _token_manager: &TokenManager<P>,
    registry: &CurveRegistry<P>,
) -> Result<PoolAttributes, ArbRsError> {
    let n_coins = tokens.len();
    let default_precision_multipliers = tokens
        .iter()
        .map(|t| U256::from(10).pow(U256::from(18 - t.decimals())))
        .collect();
    let default_rates = tokens
        .iter()
        .map(|t| U256::from(10).pow(U256::from(36 - t.decimals())))
        .collect();
    let default_use_lending = vec![false; n_coins];

    let base_pool_address = registry.get_base_pool(address).await?;
    let is_metapool = base_pool_address.is_some();

    let swap_strategy = determine_swap_strategy(address, is_metapool);

    let mut attributes = PoolAttributes {
        pool_variant: if is_metapool {
            PoolVariant::Meta
        } else {
            PoolVariant::Plain
        },
        strategy: CalculationStrategy::Legacy,
        d_variant: pool_overrides::get_d_variant(&address),
        y_variant: pool_overrides::get_y_variant(&address),
        n_coins,
        swap_strategy,
        rates: default_rates,
        precision_multipliers: default_precision_multipliers,
        use_lending: default_use_lending,
        fee_gamma: None,
        mid_fee: None,
        out_fee: None,
        offpeg_fee_multiplier: None,
        base_pool_address,
        oracle_method: None,
    };

    if ADMIN_FEE_POOLS.contains(&address) || DYNAMIC_FEE_POOLS.contains(&address) {
        attributes.d_variant = DVariant::Legacy;
    }

    println!(
        "[Attributes Builder] Applying specific overrides for {}",
        address
    );
    if UNSCALED_POOLS.contains(&address) || ADMIN_FEE_POOLS.contains(&address) {
        attributes.d_variant = DVariant::Legacy;
    }
    match address {
        SAAVE_POOL => {
            let call = offpeg_fee_multiplierCall {};
            let res_bytes = provider
                .call(
                    TransactionRequest::default()
                        .to(address)
                        .input(call.abi_encode().into()),
                )
                .await?;
            attributes.offpeg_fee_multiplier =
                Some(offpeg_fee_multiplierCall::abi_decode_returns(&res_bytes)?);
        }
        COMPOUND_POOL => {
            attributes.pool_variant = PoolVariant::Lending;
            attributes.use_lending = vec![true, true];
            attributes.precision_multipliers =
                vec![U256::from(1), U256::from(10).pow(U256::from(12))];
        }
        TRICRYPTO2_POOL => {
            attributes.fee_gamma = Some(U256::from(10_000_000_000_000_000u128));
            attributes.mid_fee = Some(U256::from(4_000_000));
            attributes.out_fee = Some(U256::from(40_000_000));
        }
        DUSD_METAPOOL => {
            attributes.precision_multipliers =
                vec![U256::from(1), U256::from(1_000_000_000_000u128)];
        }
        AAVE_POOL => {
            attributes.pool_variant = PoolVariant::Lending;
            attributes.use_lending = vec![true, true, false];
            attributes.precision_multipliers = vec![
                U256::from(1),
                U256::from(10).pow(U256::from(12)),
                U256::from(10).pow(U256::from(12)),
            ];
        }
        GUSD_METAPOOL => {
            attributes.pool_variant = PoolVariant::Lending;
            attributes.use_lending = vec![true, true, true, false];
        }
        MIM_METAPOOL => {
            attributes.precision_multipliers = vec![
                U256::from(1),
                U256::from(10).pow(U256::from(12)),
                U256::from(10).pow(U256::from(12)),
            ];
            let call = offpeg_fee_multiplierCall {};
            let res_bytes = provider
                .call(
                    TransactionRequest::default()
                        .to(address)
                        .input(call.abi_encode().into()),
                )
                .await?;
            attributes.offpeg_fee_multiplier =
                Some(offpeg_fee_multiplierCall::abi_decode_returns(&res_bytes)?);
        }
        LUSD_METAPOOL => {
            attributes.precision_multipliers = vec![
                U256::from(1),
                U256::from(10).pow(U256::from(12)),
                U256::from(10).pow(U256::from(12)),
            ];
        }
        YEARN_POOL | BUSD_YEARN_POOL => {
            attributes.pool_variant = PoolVariant::Lending;
            attributes.use_lending = vec![true, true, true, true];
            attributes.precision_multipliers = vec![
                U256::from(1),
                U256::from(10).pow(U256::from(12)),
                U256::from(10).pow(U256::from(12)),
                U256::from(1),
            ];
        }
        SUSD_POOL => {
            attributes.use_lending = vec![false, false, false, false];
        }
        RAI_METAPOOL | T_METAPOOL => {
            let call = price_oracleCall {};
            if provider
                .call(
                    TransactionRequest::default()
                        .to(address)
                        .input(call.abi_encode().into()),
                )
                .await
                .is_ok()
            {
                attributes.oracle_method = Some(1);
            } else {
                attributes.oracle_method = Some(0);
            };
        }
        _ => {
            println!("[Attributes Builder] No specific overrides for this pool.");
        }
    }
    println!("[Attributes Builder] Final attributes built successfully.");
    Ok(attributes)
}

/// Determines which swap strategy to use based on the pool's address and type.
fn determine_swap_strategy(address: Address, is_metapool: bool) -> SwapStrategyType {
    if address == TRICRYPTO2_POOL {
        SwapStrategyType::Tricrypto
    } else if DYNAMIC_FEE_POOLS.contains(&address) {
        SwapStrategyType::DynamicFee
    } else if ORACLE_POOLS.contains(&address) {
        SwapStrategyType::Oracle
    } else if ADMIN_FEE_POOLS.contains(&address) {
        SwapStrategyType::AdminFee
    } else if is_metapool {
        SwapStrategyType::Metapool
    } else if LENDING_POOLS.contains(&address) {
        SwapStrategyType::Lending
    } else if UNSCALED_POOLS.contains(&address) {
        SwapStrategyType::Unscaled
    } else {
        SwapStrategyType::Default
    }
}
