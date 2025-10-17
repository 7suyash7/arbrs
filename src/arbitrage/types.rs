use crate::core::token::Token;
use crate::errors::ArbRsError;
use crate::pool::{LiquidityPool, PoolSnapshot};
use alloy_primitives::{Address, U256};
use alloy_provider::Provider;
use std::any::Any;
use std::collections::HashMap;
use std::fmt::{self, Debug};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct SwapAction<P: Provider + Send + Sync + 'static + ?Sized> {
    pub pool_address: Address,
    pub token_in: Arc<Token<P>>,
    pub token_out: Arc<Token<P>>,
    pub amount_in: U256,
    pub min_amount_out: U256,
}

/// The final, actionable result of the arbitrage calculation.
#[derive(Debug)]
pub struct ArbitrageSolution<P: Provider + Send + Sync + 'static + ?Sized> {
    pub path: Arc<dyn Arbitrage<P>>,
    pub optimal_input: U256,
    pub gross_profit: U256,
    pub net_profit: U256,
    // <<< NEW FIELD for the canonical execution sequence >>>
    pub swap_actions: Vec<SwapAction<P>>, 
}

/// Represents a potential arbitrage opportunity, defining the sequence of pools
/// and tokens to be traded.
#[derive(Clone)]
pub struct ArbitragePath<P: Provider + Send + Sync + 'static + ?Sized> {
    pub pools: Vec<Arc<dyn LiquidityPool<P>>>,
    pub path: Vec<Arc<Token<P>>>,
    pub profit_token: Arc<Token<P>>,
}

/// A trait representing a generic arbitrage strategy.
/// The core calculation methods are synchronous and operate on pre-fetched snapshots.
pub trait Arbitrage<P: Provider + Send + Sync + 'static + ?Sized>: Debug + Send + Sync {
    /// Returns the addresses of all pools involved in the path.
    fn get_involved_pools(&self) -> Vec<Address>;

    /// Returns the pool objects involved in the path.
    fn get_pools(&self) -> &Vec<Arc<dyn LiquidityPool<P>>>;

    /// Calculates the final amount out.
    fn calculate_out_amount(
        &self,
        start_amount: U256,
        snapshots: &HashMap<Address, PoolSnapshot>,
    ) -> Result<U256, ArbRsError>;

    /// Quickly checks if a path is potentially profitable.
    fn check_viability(
        &self,
        snapshots: &HashMap<Address, PoolSnapshot>,
    ) -> Result<bool, ArbRsError>;

    /// Allows for downcasting the trait object to its concrete type.
    fn as_any(&self) -> &dyn Any;
}

#[derive(Debug)]
pub struct ProfitableOpportunity<P: Provider + Send + Sync + 'static + ?Sized> {
    pub path: Arc<dyn Arbitrage<P>>,
    pub optimal_input: U256,
    pub gross_profit: U256,
    pub net_profit: U256,
}

impl<P: Provider + Send + Sync + 'static + ?Sized> Debug for ArbitragePath<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ArbitragePath")
            .field(
                "pools",
                &self.pools.iter().map(|p| p.address()).collect::<Vec<_>>(),
            )
            .field("path", &self.path)
            .field("profit_token", &self.profit_token)
            .finish()
    }
}
