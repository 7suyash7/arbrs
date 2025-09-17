use crate::errors::ArbRsError;
use crate::math::v3::full_math;
use alloy_primitives::U256;
use std::fmt::Debug;

/// Defines the calculation logic for a Uniswap V2-like pool.
/// This allows for different fee structures or custom math for forks.
#[async_trait::async_trait]
pub trait V2CalculationStrategy: Debug + Send + Sync {
    /// Calculates the output amount for an exact input swap.
    fn calculate_tokens_out(
        &self,
        reserve_in: U256,
        reserve_out: U256,
        amount_in: U256,
    ) -> Result<U256, ArbRsError> {
        if amount_in == U256::ZERO
            || reserve_in == U256::ZERO
            || reserve_out == U256::ZERO
        {
            return Err(ArbRsError::CalculationError("Invalid input".into()));
        }

        let fee_bps = self.get_fee_bps();
        let fee_denominator = U256::from(10000);
        let fee_numerator = fee_denominator.saturating_sub(U256::from(fee_bps));

        let amount_in_with_fee = amount_in.checked_mul(fee_numerator).ok_or_else(|| {
            ArbRsError::CalculationError("Overflow calculating amount with fee".to_string())
        })?;
        let denominator = reserve_in
            .checked_mul(fee_denominator)
            .ok_or_else(|| {
                ArbRsError::CalculationError("Overflow calculating denominator".to_string())
            })?
            .checked_add(amount_in_with_fee)
            .ok_or_else(|| {
                ArbRsError::CalculationError("Overflow calculating denominator".to_string())
            })?;

        full_math::mul_div(amount_in_with_fee, reserve_out, denominator)
            .ok_or_else(|| ArbRsError::CalculationError("mul_div failed".to_string()))
    }

    /// Calculates the required input amount for an exact output swap.
    fn calculate_tokens_in_from_tokens_out(
        &self,
        reserve_in: U256,
        reserve_out: U256,
        amount_out: U256,
    ) -> Result<U256, ArbRsError> {
        if amount_out == U256::ZERO
            || reserve_in == U256::ZERO
            || reserve_out == U256::ZERO
        {
            return Err(ArbRsError::CalculationError("Invalid input".into()));
        }
        if amount_out >= reserve_out {
            return Err(ArbRsError::CalculationError(
                "Insufficient liquidity for desired output amount".to_string(),
            ));
        }

        let fee_bps = self.get_fee_bps();
        let fee_denominator = U256::from(10000);
        let fee_numerator = fee_denominator.saturating_sub(U256::from(fee_bps));

        let tmp = full_math::mul_div(
            reserve_in,
            amount_out,
            reserve_out.saturating_sub(amount_out),
        )
        .ok_or_else(|| ArbRsError::CalculationError("mul_div for tmp failed".to_string()))?;
        
        let amount_in = full_math::mul_div(tmp, fee_denominator, fee_numerator)
            .ok_or_else(|| ArbRsError::CalculationError("mul_div for final amount failed".to_string()))?;
        
        Ok(amount_in.saturating_add(U256::from(1)))
    }

    fn get_fee_bps(&self) -> u32;
}

/// Strategy for standard Uniswap V2 pools (0.3% fee).
#[derive(Debug, Clone)]
pub struct StandardV2Logic;

impl V2CalculationStrategy for StandardV2Logic {
    fn get_fee_bps(&self) -> u32 {
        30 // 30 bps = 0.3%
    }
}

/// Strategy for PancakeSwap V2 pools (0.25% fee).
#[derive(Debug, Clone)]
pub struct PancakeV2Logic;

impl V2CalculationStrategy for PancakeV2Logic {
    fn get_fee_bps(&self) -> u32 {
        25
    }
}