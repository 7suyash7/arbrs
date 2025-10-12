use crate::{
    TokenLike,
    math::balancer::fixed_point as fp,
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
use balancer_maths_rust::common::maths::{div_down_fixed, div_up_fixed, mul_down_fixed};
use balancer_maths_rust::common::maths::mul_up_fixed;
use balancer_maths_rust::common::maths::pow_up_fixed;
use balancer_maths_rust::common::maths::complement_fixed;
use num_bigint::BigInt;
use lazy_static::lazy_static;
use std::fmt::{Formatter, Result as FmtResult};
use std::{any::Any, fmt::Debug, sync::Arc};

lazy_static! {
    pub static ref WAD: BigInt = BigInt::from(10).pow(18);
}

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

#[derive(Default)]
pub struct BalancerPool<P: Provider + Send + Sync + 'static + ?Sized> {
    pub address: Address,
    provider: Arc<P>,
    tokens: Vec<Arc<Token<P>>>,
    weights: Vec<U256>,
    fee: U256,
    vault_address: Address,
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
            provider.call(TransactionRequest::default().to(address).input(IWeightedPool::getPoolIdCall {}.abi_encode().into())),
            provider.call(TransactionRequest::default().to(address).input(IWeightedPool::getVaultCall {}.abi_encode().into())),
            provider.call(TransactionRequest::default().to(address).input(IWeightedPool::getSwapFeePercentageCall {}.abi_encode().into())),
            provider.call(TransactionRequest::default().to(address).input(IWeightedPool::getNormalizedWeightsCall {}.abi_encode().into())),
        );

        let pool_id = IWeightedPool::getPoolIdCall::abi_decode_returns(&pool_id_res?)?;
        let vault_address = IWeightedPool::getVaultCall::abi_decode_returns(&vault_res?)?;
        let fee = IWeightedPool::getSwapFeePercentageCall::abi_decode_returns(&fee_res?)?;
        let weights = IWeightedPool::getNormalizedWeightsCall::abi_decode_returns(&weights_res?)?;

        let pool_tokens_bytes = provider.call(TransactionRequest::default().to(vault_address).input(IVault::getPoolTokensCall { poolId: pool_id }.abi_encode().into())).await?;
        let pool_tokens_res = IVault::getPoolTokensCall::abi_decode_returns(&pool_tokens_bytes)?;
        let token_addresses = pool_tokens_res.tokens;

        let token_futs = token_addresses.into_iter().map(|addr| token_manager.get_token(addr));
        let tokens: Vec<_> = futures::future::join_all(token_futs).await.into_iter().collect::<Result<_, _>>()?;

        Ok(Self {
            address,
            provider,
            tokens,
            weights,
            fee,
            vault_address,
            pool_id: pool_id.0,
        })
    }
    
    pub fn fee(&self) -> U256 { self.fee }
    pub fn weights(&self) -> &Vec<U256> { &self.weights }
}

#[async_trait]
impl<P: Provider + Send + Sync + 'static + ?Sized> LiquidityPool<P> for BalancerPool<P> {
    fn address(&self) -> Address { self.address }
    fn get_all_tokens(&self) -> Vec<Arc<Token<P>>> { self.tokens.clone() }
    fn as_any(&self) -> &dyn Any { self }
    
    async fn update_state(&self) -> Result<(), ArbRsError> {
        Ok(())
    }

    async fn get_snapshot(&self, block_number: Option<u64>) -> Result<PoolSnapshot, ArbRsError> {
        let call = IVault::getPoolTokensCall { poolId: self.pool_id.into() };
        let request = TransactionRequest::default().to(self.vault_address).input(call.abi_encode().into());
        let result_bytes = self.provider.call(request).block(block_number.map(BlockId::from).unwrap_or(BlockId::latest())).await?;
        let pool_tokens_res = IVault::getPoolTokensCall::abi_decode_returns(&result_bytes)?;

        let snapshot = BalancerPoolSnapshot { balances: pool_tokens_res.balances };
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
            _ => return Err(ArbRsError::CalculationError("Invalid snapshot for Balancer pool".into())),
        };

        let token_in_index = self.tokens.iter().position(|t| t.address() == token_in.address()).unwrap();
        let token_out_index = self.tokens.iter().position(|t| t.address() == token_out.address()).unwrap();

        let balance_in = fp::to_bigint(balancer_snapshot.balances[token_in_index]);
        let balance_out = fp::to_bigint(balancer_snapshot.balances[token_out_index]);
        let weight_in = fp::to_bigint(self.weights[token_in_index]);
        let weight_out = fp::to_bigint(self.weights[token_out_index]);
        let amount_in = fp::to_bigint(amount_in);
        let fee = fp::to_bigint(self.fee);

        let scaling_factor_in = BigInt::from(10).pow(18 - self.tokens[token_in_index].decimals() as u32);
        let scaling_factor_out = BigInt::from(10).pow(18 - self.tokens[token_out_index].decimals() as u32);

        let scaled_balance_in = balance_in * &scaling_factor_in;
        let scaled_balance_out = balance_out * &scaling_factor_out;
        let scaled_amount_in = amount_in * &scaling_factor_in;

        // let fee_amount = mul_up_fixed(&scaled_amount_in, &fee)?;
        // let amount_in_after_fee = &scaled_amount_in - fee_amount;
        let amount_in_after_fee = mul_down_fixed(&scaled_amount_in, &(&*WAD - fee))?;

        let denominator = &scaled_balance_in + &amount_in_after_fee;
        let base = div_up_fixed(&scaled_balance_in, &denominator)?;
        let exponent = div_down_fixed(&weight_in, &weight_out)?;
        let power = pow_up_fixed(&base, &exponent)?;

        let scaled_amount_out = mul_down_fixed(&scaled_balance_out, &complement_fixed(&power)?)?;

        fp::to_u256(scaled_amount_out / scaling_factor_out)
    }

    fn calculate_tokens_in(&self, token_in: &Token<P>, token_out: &Token<P>, amount_out: U256, snapshot: &PoolSnapshot) -> Result<U256, ArbRsError> {
        let balancer_snapshot = match snapshot {
            PoolSnapshot::Balancer(s) => s,
            _ => return Err(ArbRsError::CalculationError("Invalid snapshot for Balancer pool".into())),
        };

        let token_in_index = self.tokens.iter().position(|t| t.address() == token_in.address()).unwrap();
        let token_out_index = self.tokens.iter().position(|t| t.address() == token_out.address()).unwrap();

        let scaling_factor_in = BigInt::from(10).pow(18 - self.tokens[token_in_index].decimals() as u32);
        let scaling_factor_out = BigInt::from(10).pow(18 - self.tokens[token_out_index].decimals() as u32);
        
        let scaled_balance_in = fp::to_bigint(balancer_snapshot.balances[token_in_index]) * &scaling_factor_in;
        let scaled_balance_out = fp::to_bigint(balancer_snapshot.balances[token_out_index]) * &scaling_factor_out;
        let scaled_amount_out = fp::to_bigint(amount_out) * &scaling_factor_out;

        let scaled_amount_in_before_fee = balancer_maths_rust::pools::weighted::compute_in_given_exact_out(
            &scaled_balance_in,
            &fp::to_bigint(self.weights[token_in_index]),
            &scaled_balance_out,
            &fp::to_bigint(self.weights[token_out_index]),
            &scaled_amount_out,
        )?;

        let fee_bigint = fp::to_bigint(self.fee);
        let one_wad = BigInt::from(10).pow(18);
        let amount_in_with_fee = (&scaled_amount_in_before_fee * &one_wad) / (&one_wad - fee_bigint);

        fp::to_u256((amount_in_with_fee + BigInt::from(1)) / scaling_factor_in)
    }

    async fn nominal_price(&self, _t_in: &Token<P>, _t_out: &Token<P>) -> Result<f64, ArbRsError> { unimplemented!() }
    async fn absolute_price(&self, _t_in: &Token<P>, _t_out: &Token<P>) -> Result<f64, ArbRsError> { unimplemented!() }
    async fn absolute_exchange_rate(&self, _t_in: &Token<P>, _t_out: &Token<P>) -> Result<f64, ArbRsError> { unimplemented!() }
}

impl<P: Provider + Send + Sync + 'static + ?Sized> Debug for BalancerPool<P> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.debug_struct("BalancerPool")
            .field("address", &self.address)
            .field("vault", &self.vault_address)
            .field("tokens", &self.tokens.iter().map(|t| t.symbol()).collect::<Vec<_>>())
            .field("fee", &self.fee)
            .finish()
    }
}
