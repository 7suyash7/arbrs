use crate::{
    arbitrage::types::{Arbitrage, ArbitragePath},
    balancer::pool::BalancerPool,
    core::token::TokenLike,
    curve::{
        constants::FEE_DENOMINATOR, pool::CurveStableswapPool, pool_attributes::SwapStrategyType,
    },
    errors::ArbRsError,
    math::{utils::u256_to_f64, v3::constants::Q96},
    pool::{LiquidityPool, PoolSnapshot, uniswap_v3::UniswapV3Pool},
};
use alloy_primitives::{Address, U256};
use alloy_provider::Provider;
use std::{
    any::Any,
    collections::HashMap,
    fmt::{self, Debug, Formatter},
    sync::Arc,
};

/// Represents a simple arbitrage cycle through one or more pools. (e.g., WETH -> USDC -> WETH).
#[derive(Clone)]
pub struct ArbitrageCycle<P: Provider + Send + Sync + 'static + ?Sized> {
    pub path: Arc<ArbitragePath<P>>,
}

impl<P: Provider + Send + Sync + 'static + ?Sized> ArbitrageCycle<P> {
    pub fn new(path: ArbitragePath<P>) -> Self {
        Self {
            path: Arc::new(path),
        }
    }
}

impl<P: Provider + Send + Sync + 'static + ?Sized> Arbitrage<P> for ArbitrageCycle<P> {
    fn get_involved_pools(&self) -> Vec<Address> {
        self.path.pools.iter().map(|p| p.address()).collect()
    }

    fn get_pools(&self) -> &Vec<Arc<dyn LiquidityPool<P>>> {
        &self.path.pools
    }

    fn calculate_out_amount(
        &self,
        start_amount: U256,
        snapshots: &HashMap<Address, PoolSnapshot>,
    ) -> Result<U256, ArbRsError> {
        if start_amount.is_zero() {
            return Ok(U256::ZERO);
        }
        let mut current_amount = start_amount;

        for i in 0..self.path.pools.len() {
            let pool = &self.path.pools[i];
            let snapshot = snapshots
                .get(&pool.address())
                .ok_or(ArbRsError::NoPoolStateAvailable(0))?;

            let token_in = &self.path.path[i];
            let token_out = &self.path.path[i + 1];

            current_amount =
                pool.calculate_tokens_out(token_in, token_out, current_amount, snapshot)?;

            if current_amount.is_zero() {
                break;
            }
        }
        Ok(current_amount)
    }

    fn check_viability(
        &self,
        snapshots: &HashMap<Address, PoolSnapshot>,
    ) -> Result<bool, ArbRsError> {
        let mut profit_factor = 1.0;

        for i in 0..self.path.pools.len() {
            let pool_arc = &self.path.pools[i];
            let snapshot = snapshots
                .get(&pool_arc.address())
                .ok_or(ArbRsError::NoPoolStateAvailable(0))?;

            let token_in = &self.path.path[i];
            let token_out = &self.path.path[i + 1];

            let (price, fee_factor) = match snapshot {
                PoolSnapshot::UniswapV2(s) => {
                    if s.reserve0.is_zero() {
                        return Ok(false);
                    }
                    let (reserve_in, reserve_out) = if *pool_arc.get_all_tokens()[0] == **token_in {
                        (s.reserve0, s.reserve1)
                    } else {
                        (s.reserve1, s.reserve0)
                    };
                    (u256_to_f64(reserve_out) / u256_to_f64(reserve_in), 0.997)
                }
                PoolSnapshot::UniswapV3(s) => {
                    if s.sqrt_price_x96.is_zero() {
                        return Ok(false);
                    }
                    let ratio = u256_to_f64(s.sqrt_price_x96) / u256_to_f64(Q96);
                    let price_of_token0_in_token1 = ratio.powi(2);
                    let price = if *pool_arc.get_all_tokens()[0] == **token_in {
                        price_of_token0_in_token1
                    } else {
                        1.0 / price_of_token0_in_token1
                    };

                    let fee = pool_arc
                        .as_any()
                        .downcast_ref::<UniswapV3Pool<P>>()
                        .unwrap()
                        .fee();
                    (price, 1.0 - (fee as f64 / 1_000_000.0))
                }
                PoolSnapshot::Curve(s) => {
                    let curve_pool = pool_arc
                        .as_any()
                        .downcast_ref::<CurveStableswapPool<P>>()
                        .unwrap();
                    let fee_factor = 1.0 - (u256_to_f64(s.fee) / u256_to_f64(FEE_DENOMINATOR));

                    let price = match curve_pool.attributes.swap_strategy {
                        SwapStrategyType::Default
                        | SwapStrategyType::Metapool
                        | SwapStrategyType::Lending => {
                            10f64.powi(token_in.decimals() as i32 - token_out.decimals() as i32)
                        }
                        _ => {
                            let i = curve_pool
                                .tokens
                                .iter()
                                .position(|t| t == token_in)
                                .unwrap();
                            let j = curve_pool
                                .tokens
                                .iter()
                                .position(|t| t == token_out)
                                .unwrap();
                            if s.balances.is_empty() || s.balances[i].is_zero() {
                                return Ok(false);
                            }
                            let reserve_in =
                                u256_to_f64(s.balances[i]) / 10f64.powi(token_in.decimals() as i32);
                            let reserve_out = u256_to_f64(s.balances[j])
                                / 10f64.powi(token_out.decimals() as i32);
                            reserve_out / reserve_in
                        }
                    };
                    (price, fee_factor)
                }

                PoolSnapshot::Balancer(s) => {
                    let balancer_pool =
                        pool_arc.as_any().downcast_ref::<BalancerPool<P>>().unwrap();
                    let fee_factor = 1.0 - (u256_to_f64(balancer_pool.fee()) / 1e18);

                    let tokens = pool_arc.get_all_tokens();
                    let i = tokens.iter().position(|t| **t == **token_in).unwrap();
                    let j = tokens.iter().position(|t| **t == **token_out).unwrap();

                    let balance_in = u256_to_f64(s.balances[i]);
                    let weight_in = u256_to_f64(balancer_pool.weights()[i]);

                    let balance_out = u256_to_f64(s.balances[j]);
                    let weight_out = u256_to_f64(balancer_pool.weights()[j]);

                    if balance_in == 0.0 || weight_in == 0.0 {
                        return Ok(false);
                    }

                    let price = (balance_out / weight_out) / (balance_in / weight_in);

                    (price, fee_factor)
                }
            };

            profit_factor *= price * fee_factor;
        }

        Ok(profit_factor > 1.0)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl<P: Provider + Send + Sync + 'static + ?Sized> Debug for ArbitrageCycle<P> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("ArbitrageCycle")
            .field("path", &self.path)
            .finish()
    }
}
