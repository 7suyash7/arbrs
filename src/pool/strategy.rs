use crate::errors::ArbRsError;
use alloy_primitives::U256;
use std::fmt::Debug;

/// Defines the calculation logic for a Uniswap V2-like pool.
/// This allows for different fee structures or custom math for forks.
#[async_trait::async_trait]
pub trait V2CalculationStrategy: Debug + Send + Sync {
    /// Calculates the output amount for an exact input swap.
    ///
    /// # Arguments
    /// * `reserve_in`: Reserve of the input token.
    /// * `reserve_out`: Reserve of the output token.
    /// * `amount_in`: Amount of input token.
    ///
    /// Default implementation uses the standard constant product formula.
    fn calculate_tokens_out(
        &self,
        reserve_in: U256,
        reserve_out: U256,
        amount_in: U256,
    ) -> Result<U256, ArbRsError> {
        if amount_in == U256::ZERO {
            return Err(ArbRsError::CalculationError(
                "Input amount cannot be zero".to_string(),
            ));
        }
        if reserve_in == U256::ZERO || reserve_out == U256::ZERO {
            return Err(ArbRsError::CalculationError(
                "Pool reserves cannot be zero".to_string(),
            ));
        }

        // Standard V2 fee calculation: amount_in * fee / 10000
        // fee_bps = 30 for standard 0.3% fee (30 basis points = 0.3%)
        let fee_bps = self.get_fee_bps();
        let fee_denominator = U256::from(10000);
        let fee_numerator = U256::from(fee_bps);

        let amount_in_with_fee = amount_in
            .checked_mul(fee_denominator.saturating_sub(fee_numerator))
            .ok_or_else(|| {
                ArbRsError::CalculationError("Overflow calculating amount with fee".to_string())
            })?;
        let numerator = amount_in_with_fee.checked_mul(reserve_out).ok_or_else(|| {
            ArbRsError::CalculationError("Overflow calculating numerator".to_string())
        })?;
        let denominator = reserve_in
            .checked_mul(fee_denominator)
            .ok_or_else(|| {
                ArbRsError::CalculationError("Overflow calculating denominator part 1".to_string())
            })?
            .checked_add(amount_in_with_fee)
            .ok_or_else(|| {
                ArbRsError::CalculationError("Overflow calculating denominator part 2".to_string())
            })?;

        if denominator == U256::ZERO {
            return Err(ArbRsError::CalculationError(
                "Division by zero in calculation".to_string(),
            ));
        }
        Ok(numerator / denominator)
    }

    /// Calculates the required input amount for an exact output swap.
    fn calculate_tokens_in_from_tokens_out(
        &self,
        reserve_in: U256,
        reserve_out: U256,
        amount_out: U256,
    ) -> Result<U256, ArbRsError> {
        if amount_out == U256::ZERO {
            return Err(ArbRsError::CalculationError(
                "Output amount cannot be zero".to_string(),
            ));
        }
        if reserve_in == U256::ZERO || reserve_out == U256::ZERO {
            return Err(ArbRsError::CalculationError(
                "Pool reserves cannot be zero".to_string(),
            ));
        }
        if amount_out >= reserve_out {
            return Err(ArbRsError::CalculationError(
                "Insufficient liquidity for desired output amount".to_string(),
            ));
        }

        let fee_bps = self.get_fee_bps();
        let fee_denominator = U256::from(10000);
        let fee_numerator = U256::from(fee_bps);

        let numerator = reserve_in
            .checked_mul(amount_out)
            .ok_or_else(|| {
                ArbRsError::CalculationError("Overflow calculating numerator part 1".to_string())
            })?
            .checked_mul(fee_denominator)
            .ok_or_else(|| {
                ArbRsError::CalculationError("Overflow calculating numerator part 2".to_string())
            })?;
        let denominator = reserve_out
            .checked_sub(amount_out)
            .ok_or_else(|| {
                ArbRsError::CalculationError("Underflow calculating denominator part 1".to_string())
            })?
            .checked_mul(fee_denominator.saturating_sub(fee_numerator))
            .ok_or_else(|| {
                ArbRsError::CalculationError("Overflow calculating denominator part 2".to_string())
            })?;

        if denominator == U256::ZERO {
            return Err(ArbRsError::CalculationError(
                "Division by zero in calculation".to_string(),
            ));
        }

        let amount_in = numerator
            .checked_div(denominator)
            .ok_or_else(|| {
                ArbRsError::CalculationError("Division error during final calculation".to_string())
            })?
            .checked_add(U256::from(1))
            .ok_or_else(|| {
                ArbRsError::CalculationError("Overflow adding safety margin".to_string())
            })?;
        Ok(amount_in)
    }

    /// Returns the fee in basis points (bps). E.g., 30 bps for 0.3%.
    fn get_fee_bps(&self) -> u32;

    // logic for complex forks like Camelot.
    // fn custom_logic(&self) -> Result<(), ArbRsError> { Ok(()) }
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
        25 // 25 bps = 0.25%
    }
}

// again can add more v2 forks here which I will add later.
