use crate::{
    TokenLike,
    balancer::{scaling_helper, weighted_math},
    core::token::Token,
    db::DbManager,
    errors::ArbRsError,
    manager::token_manager::TokenManager,
    pool::{LiquidityPool, PoolSnapshot},
};
use alloy_primitives::{Address, U256};
use alloy_provider::Provider;
use alloy_rpc_types::{BlockId, TransactionRequest};
use alloy_sol_types::{SolCall, sol};
use async_trait::async_trait;
use std::fmt::{Formatter, Result as FmtResult};
use std::{any::Any, fmt::Debug, sync::Arc};

sol! {
    contract IVault {
        function getPoolTokens(bytes32 poolId) external view returns (address[] tokens, uint256[] balances, uint256 lastChangeBlock);
    }
    contract IWeightedPool {
        function getPoolId() external view returns (bytes32);
        function getVault() external view returns (address);
        function getSwapFeePercentage() external view returns (uint256);
        function getNormalizedWeights() external view returns (uint256[]);
    }
}

#[derive(Clone, Debug, Default)]
pub struct BalancerPoolSnapshot {
    pub balances: Vec<U256>,
}

pub struct BalancerPool<P: Provider + Send + Sync + 'static + ?Sized> {
    pub address: Address,
    provider: Arc<P>,
    tokens: Vec<Arc<Token<P>>>,
    scaling_factors: Vec<U256>,
    weights: Vec<U256>,
    fee: U256,
    vault: Address,
    pub pool_id: [u8; 32],
}

impl<P: Provider + Send + Sync + 'static + ?Sized> BalancerPool<P> {
    pub async fn new(
        address: Address,
        provider: Arc<P>,
        token_manager: Arc<TokenManager<P>>,
        _db_manager: Arc<DbManager>,
    ) -> Result<Self, ArbRsError> {
        let (pool_id_res, vault_res, fee_res, weights_res) = tokio::join!(
            provider.call(
                TransactionRequest::default()
                    .to(address)
                    .input(IWeightedPool::getPoolIdCall {}.abi_encode().into())
            ),
            provider.call(
                TransactionRequest::default()
                    .to(address)
                    .input(IWeightedPool::getVaultCall {}.abi_encode().into())
            ),
            provider.call(
                TransactionRequest::default().to(address).input(
                    IWeightedPool::getSwapFeePercentageCall {}
                        .abi_encode()
                        .into()
                )
            ),
            provider.call(
                TransactionRequest::default().to(address).input(
                    IWeightedPool::getNormalizedWeightsCall {}
                        .abi_encode()
                        .into()
                )
            ),
        );
        let pool_id = IWeightedPool::getPoolIdCall::abi_decode_returns(&pool_id_res?)?;
        let vault = IWeightedPool::getVaultCall::abi_decode_returns(&vault_res?)?;
        let fee = IWeightedPool::getSwapFeePercentageCall::abi_decode_returns(&fee_res?)?;
        let weights = IWeightedPool::getNormalizedWeightsCall::abi_decode_returns(&weights_res?)?;

        let pool_tokens_bytes = provider
            .call(
                TransactionRequest::default().to(vault).input(
                    IVault::getPoolTokensCall { poolId: pool_id }
                        .abi_encode()
                        .into(),
                ),
            )
            .await?;
        let pool_tokens_res = IVault::getPoolTokensCall::abi_decode_returns(&pool_tokens_bytes)?;
        let token_addresses = pool_tokens_res.tokens;

        let token_futs = token_addresses
            .into_iter()
            .map(|addr| token_manager.get_token(addr));
        let tokens: Vec<_> = futures::future::join_all(token_futs)
            .await
            .into_iter()
            .collect::<Result<_, _>>()?;
        let scaling_factors = tokens
            .iter()
            .map(scaling_helper::compute_scaling_factor)
            .collect();

        Ok(Self {
            address,
            provider,
            tokens,
            scaling_factors,
            weights,
            fee,
            vault,
            pool_id: pool_id.0,
        })
    }
    pub fn fee(&self) -> U256 {
        self.fee
    }

    pub fn weights(&self) -> &Vec<U256> {
        &self.weights
    }
}

#[async_trait]
impl<P: Provider + Send + Sync + 'static + ?Sized> LiquidityPool<P> for BalancerPool<P> {
    fn address(&self) -> Address {
        self.address
    }
    fn get_all_tokens(&self) -> Vec<Arc<Token<P>>> {
        self.tokens.clone()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    async fn update_state(&self) -> Result<(), ArbRsError> {
        // Balancer state is fetched on-demand in get_snapshot, so this can be a no-op
        Ok(())
    }

    async fn get_snapshot(&self, block_number: Option<u64>) -> Result<PoolSnapshot, ArbRsError> {
        let call = IVault::getPoolTokensCall {
            poolId: self.pool_id.into(),
        };
        let request = TransactionRequest::default()
            .to(self.vault)
            .input(call.abi_encode().into());
        let result_bytes = self
            .provider
            .call(request)
            .block(block_number.map(BlockId::from).unwrap_or(BlockId::latest()))
            .await?;
        let pool_tokens_res = IVault::getPoolTokensCall::abi_decode_returns(&result_bytes)?;

        let snapshot = BalancerPoolSnapshot {
            balances: pool_tokens_res.balances,
        };
        Ok(PoolSnapshot::Balancer(snapshot))
    }

    fn calculate_tokens_out(
        &self,
        token_in: &Token<P>,
        token_out: &Token<P>,
        amount_in: U256,
        snapshot: &PoolSnapshot,
    ) -> Result<U256, ArbRsError> {
        let balancer_snapshot = match snapshot {
            PoolSnapshot::Balancer(s) => s,
            _ => {
                return Err(ArbRsError::CalculationError(
                    "Invalid snapshot for Balancer pool".into(),
                ));
            }
        };

        let i = self
            .tokens
            .iter()
            .position(|t| **t == *token_in)
            .ok_or(ArbRsError::CalculationError("Token In not found".into()))?;
        let j = self
            .tokens
            .iter()
            .position(|t| **t == *token_out)
            .ok_or(ArbRsError::CalculationError("Token Out not found".into()))?;

        let amount_in_after_fee = weighted_math::subtract_swap_fee_amount(amount_in, self.fee)?;

        let mut upscaled_balances = balancer_snapshot.balances.clone();
        for k in 0..self.tokens.len() {
            upscaled_balances[k] =
                scaling_helper::upscale(upscaled_balances[k], self.scaling_factors[k])?;
        }
        let upscaled_amount_in =
            scaling_helper::upscale(amount_in_after_fee, self.scaling_factors[i])?;

        let upscaled_amount_out = weighted_math::calc_out_given_in(
            upscaled_balances[i],
            self.weights[i],
            upscaled_balances[j],
            self.weights[j],
            upscaled_amount_in,
        )?;

        scaling_helper::downscale_down(upscaled_amount_out, self.scaling_factors[j])
    }

    fn calculate_tokens_in(
        &self,
        _token_in: &Token<P>,
        _token_out: &Token<P>,
        _amount_out: U256,
        _snapshot: &PoolSnapshot,
    ) -> Result<U256, ArbRsError> {
        unimplemented!("Balancer calculate_tokens_in not yet implemented");
    }

    async fn nominal_price(&self, _t_in: &Token<P>, _t_out: &Token<P>) -> Result<f64, ArbRsError> {
        unimplemented!()
    }
    async fn absolute_price(&self, _t_in: &Token<P>, _t_out: &Token<P>) -> Result<f64, ArbRsError> {
        unimplemented!()
    }
    async fn absolute_exchange_rate(
        &self,
        _t_in: &Token<P>,
        _t_out: &Token<P>,
    ) -> Result<f64, ArbRsError> {
        unimplemented!()
    }
}

impl<P: Provider + Send + Sync + 'static + ?Sized> Debug for BalancerPool<P> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.debug_struct("BalancerPool")
            .field("address", &self.address)
            .field("vault", &self.vault)
            .field(
                "tokens",
                &self.tokens.iter().map(|t| t.symbol()).collect::<Vec<_>>(),
            )
            .field("fee", &self.fee)
            .finish()
    }
}
