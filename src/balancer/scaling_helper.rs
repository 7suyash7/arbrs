use crate::{
    core::token::Token,
    errors::ArbRsError,
    math::balancer::fixed_point as fp,
    TokenLike,
};
use alloy_primitives::U256;
use alloy_provider::Provider;
use std::sync::Arc;

pub fn compute_scaling_factor<P>(token: &Arc<Token<P>>) -> U256
where
    P: Provider + Send + Sync + 'static + ?Sized,
{
    let decimals_diff = 18 - token.decimals();
    U256::from(10).pow(U256::from(decimals_diff))
}

pub fn upscale(amount: U256, scaling_factor: U256) -> Result<U256, ArbRsError> {
    fp::mul_down(amount, scaling_factor)
}

pub fn downscale_down(amount: U256, scaling_factor: U256) -> Result<U256, ArbRsError> {
    fp::div_down(amount, scaling_factor)
}

pub fn downscale_up(amount: U256, scaling_factor: U256) -> Result<U256, ArbRsError> {
    fp::div_up(amount, scaling_factor)
}