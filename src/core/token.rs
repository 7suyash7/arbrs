use crate::errors::ArbRsError;
use alloy_primitives::{Address, Bytes, TxKind, U256};
use alloy_provider::Provider;
use alloy_rpc_types::{BlockId, BlockNumberOrTag, TransactionRequest};
use alloy_sol_types::{SolCall, sol};
use async_trait::async_trait;
use lru::LruCache;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::hash::{Hash, Hasher};
use std::num::NonZeroUsize;
use std::sync::Arc;
use tokio::sync::Mutex;

sol!(
    function allowance(address owner, address spender) external view returns (uint256);
    function totalSupply() external view returns (uint256);
    function balanceOf(address owner) external view returns (uint256 balance);
);

const BALANCE_CACHE_SIZE: usize = 256;

#[async_trait]
pub trait TokenLike: Send + Sync {
    fn address(&self) -> Address;
    fn symbol(&self) -> &str;
    fn decimals(&self) -> u8;

    async fn get_balance(
        &self,
        owner: Address,
        block_number: Option<u64>,
    ) -> Result<U256, ArbRsError>;

    async fn get_allowance(
        &self,
        owner: Address,
        spender: Address,
        block_number: Option<u64>,
    ) -> Result<U256, ArbRsError>;

    async fn get_total_supply(&self, block_number: Option<u64>) -> Result<U256, ArbRsError>;
}

pub struct NativeTokenData<P: ?Sized> {
    pub chain_id: u64,
    pub symbol: String,
    pub placeholder_address: Address,
    provider: Arc<P>,
    balance_cache: Arc<Mutex<LruCache<u64, U256>>>,
}

impl<P: ?Sized> Debug for NativeTokenData<P> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.debug_struct("NativeTokenData")
            .field("chain_id", &self.chain_id)
            .field("symbol", &self.symbol)
            .field("placeholder_address", &self.placeholder_address)
            .finish_non_exhaustive()
    }
}

impl<P: Provider + Send + Sync + ?Sized> NativeTokenData<P> {
    pub fn new(chain_id: u64, placeholder_address: Address, provider: Arc<P>) -> Self {
        Self {
            chain_id,
            symbol: "ETH".to_string(),
            placeholder_address,
            provider,
            balance_cache: Arc::new(Mutex::new(LruCache::new(
                NonZeroUsize::new(BALANCE_CACHE_SIZE).unwrap(),
            ))),
        }
    }
}

#[async_trait]
impl<P: Provider + Send + Sync + 'static + ?Sized> TokenLike for NativeTokenData<P> {
    fn address(&self) -> Address {
        self.placeholder_address
    }
    fn symbol(&self) -> &str {
        &self.symbol
    }
    fn decimals(&self) -> u8 {
        18
    }

    async fn get_balance(
        &self,
        owner: Address,
        block_number: Option<u64>,
    ) -> Result<U256, ArbRsError> {
        let block_id = match block_number {
            Some(num) => BlockNumberOrTag::Number(num),
            None => BlockNumberOrTag::Latest,
        };

        if let Some(num) = block_number {
            let mut cache = self.balance_cache.lock().await;
            if let Some(balance) = cache.get(&num) {
                return Ok(*balance);
            }
        }

        let balance = self
            .provider
            .get_balance(owner)
            .block_id(block_id.into())
            .await
            .map_err(|e| ArbRsError::ProviderError(e.to_string()))?;

        if let Some(num) = block_number {
            let mut cache = self.balance_cache.lock().await;
            cache.put(num, balance);
        }
        Ok(balance)
    }

    async fn get_allowance(
        &self,
        _owner: Address,
        _spender: Address,
        _block_number: Option<u64>,
    ) -> Result<U256, ArbRsError> {
        Ok(U256::MAX)
    }

    async fn get_total_supply(&self, _block_number: Option<u64>) -> Result<U256, ArbRsError> {
        Ok(U256::ZERO)
    }
}

pub struct Erc20Data<P: ?Sized> {
    pub address: Address,
    pub symbol: String,
    pub name: String,
    pub decimals: u8,
    pub provider: Arc<P>,
    pub balances: Arc<Mutex<HashMap<Address, Arc<Mutex<LruCache<u64, U256>>>>>>,
    pub total_supply_cache: Arc<Mutex<LruCache<u64, U256>>>,
    pub allowance_cache:
        Arc<Mutex<HashMap<Address, HashMap<Address, Arc<Mutex<LruCache<u64, U256>>>>>>>,
}

impl<P: ?Sized> Debug for Erc20Data<P> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.debug_struct("Erc20Data")
            .field("address", &self.address)
            .field("symbol", &self.symbol)
            .field("name", &self.name)
            .field("decimals", &self.decimals)
            .finish_non_exhaustive()
    }
}

impl<P: Provider + Send + Sync + ?Sized> Erc20Data<P> {
    pub fn new(
        address: Address,
        symbol: String,
        name: String,
        decimals: u8,
        provider: Arc<P>,
    ) -> Self {
        Self {
            address,
            symbol,
            name,
            decimals,
            provider,
            balances: Arc::new(Mutex::new(HashMap::new())),
            total_supply_cache: Arc::new(Mutex::new(LruCache::new(
                NonZeroUsize::new(BALANCE_CACHE_SIZE).unwrap(),
            ))),
            allowance_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl<P: Provider + Send + Sync + 'static + ?Sized> TokenLike for Erc20Data<P> {
    fn address(&self) -> Address {
        self.address
    }
    fn symbol(&self) -> &str {
        &self.symbol
    }
    fn decimals(&self) -> u8 {
        self.decimals
    }

    async fn get_balance(
        &self,
        owner: Address,
        block_number: Option<u64>,
    ) -> Result<U256, ArbRsError> {
        let block_for_call: BlockId = match block_number {
            Some(num) => BlockNumberOrTag::Number(num).into(),
            None => BlockNumberOrTag::Latest.into(),
        };

        let block_for_cache = if let Some(num) = block_number {
            num
        } else {
            self.provider
                .get_block_number()
                .await
                .map_err(|e| ArbRsError::ProviderError(e.to_string()))?
        };

        let owner_cache = {
            let mut balances_map = self.balances.lock().await;
            balances_map
                .entry(owner)
                .or_insert_with(|| {
                    Arc::new(Mutex::new(LruCache::new(
                        NonZeroUsize::new(BALANCE_CACHE_SIZE).unwrap(),
                    )))
                })
                .clone()
        };

        {
            let mut cache = owner_cache.lock().await;
            if let Some(balance) = cache.get(&block_for_cache) {
                return Ok(*balance);
            }
        }

        let call = balanceOfCall { owner };
        let request = TransactionRequest {
            to: Some(TxKind::Call(self.address)),
            input: Some(Bytes::from(call.abi_encode())).into(),
            ..Default::default()
        };

        let result_bytes = self
            .provider
            .call(request)
            .block(block_for_call)
            .await
            .map_err(|e| ArbRsError::ProviderError(e.to_string()))?;

        let decoded_result = balanceOfCall::abi_decode_returns(&result_bytes)
            .map_err(|e| ArbRsError::AbiDecodeError(e.to_string()))?;
        let balance = decoded_result;

        let mut cache = owner_cache.lock().await;
        cache.put(block_for_cache, balance);

        Ok(balance)
    }

    async fn get_total_supply(&self, block_number: Option<u64>) -> Result<U256, ArbRsError> {
        let block_for_call: BlockId = match block_number {
            Some(num) => BlockNumberOrTag::Number(num).into(),
            None => BlockNumberOrTag::Latest.into(),
        };
        let block_for_cache = if let Some(num) = block_number {
            num
        } else {
            self.provider
                .get_block_number()
                .await
                .map_err(|e| ArbRsError::ProviderError(e.to_string()))?
        };

        {
            let mut cache = self.total_supply_cache.lock().await;
            if let Some(supply) = cache.get(&block_for_cache) {
                return Ok(*supply);
            }
        }

        let call = totalSupplyCall {};
        let request = TransactionRequest {
            to: Some(TxKind::Call(self.address)),
            input: Some(Bytes::from(call.abi_encode())).into(),
            ..Default::default()
        };
        let result_bytes = self
            .provider
            .call(request)
            .block(block_for_call)
            .await
            .map_err(|e| ArbRsError::ProviderError(e.to_string()))?;
        let total_supply = totalSupplyCall::abi_decode_returns(&result_bytes)
            .map_err(|e| ArbRsError::AbiDecodeError(e.to_string()))?;

        self.total_supply_cache
            .lock()
            .await
            .put(block_for_cache, total_supply);
        Ok(total_supply)
    }

    async fn get_allowance(
        &self,
        owner: Address,
        spender: Address,
        block_number: Option<u64>,
    ) -> Result<U256, ArbRsError> {
        let block_for_call: BlockId = match block_number {
            Some(num) => BlockNumberOrTag::Number(num).into(),
            None => BlockNumberOrTag::Latest.into(),
        };
        let block_for_cache = if let Some(num) = block_number {
            num
        } else {
            self.provider
                .get_block_number()
                .await
                .map_err(|e| ArbRsError::ProviderError(e.to_string()))?
        };

        let spender_cache = {
            let mut owner_map = self.allowance_cache.lock().await;
            owner_map
                .entry(owner)
                .or_insert_with(HashMap::new)
                .entry(spender)
                .or_insert_with(|| {
                    Arc::new(Mutex::new(LruCache::new(
                        NonZeroUsize::new(BALANCE_CACHE_SIZE).unwrap(),
                    )))
                })
                .clone()
        };
        {
            let mut cache = spender_cache.lock().await;
            if let Some(allowance) = cache.get(&block_for_cache) {
                return Ok(*allowance);
            }
        }

        let call = allowanceCall { owner, spender };
        let request = TransactionRequest {
            to: Some(TxKind::Call(self.address)),
            input: Some(Bytes::from(call.abi_encode())).into(),
            ..Default::default()
        };
        let result_bytes = self
            .provider
            .call(request)
            .block(block_for_call)
            .await
            .map_err(|e| ArbRsError::ProviderError(e.to_string()))?;
        let allowance = allowanceCall::abi_decode_returns(&result_bytes)
            .map_err(|e| ArbRsError::AbiDecodeError(e.to_string()))?;

        spender_cache.lock().await.put(block_for_cache, allowance);
        Ok(allowance)
    }
}

#[derive(Clone)]
pub enum Token<P: ?Sized> {
    Erc20(Arc<Erc20Data<P>>),
    Native(Arc<NativeTokenData<P>>),
}

#[async_trait]
impl<P: Provider + Send + Sync + 'static + ?Sized> TokenLike for Token<P> {
    fn address(&self) -> Address {
        match self {
            Token::Erc20(token) => token.address(),
            Token::Native(token) => token.address(),
        }
    }
    fn symbol(&self) -> &str {
        match self {
            Token::Erc20(token) => token.symbol(),
            Token::Native(token) => token.symbol(),
        }
    }
    fn decimals(&self) -> u8 {
        match self {
            Token::Erc20(token) => token.decimals(),
            Token::Native(token) => token.decimals(),
        }
    }
    async fn get_balance(
        &self,
        owner: Address,
        block_number: Option<u64>,
    ) -> Result<U256, ArbRsError> {
        match self {
            Token::Erc20(token) => token.get_balance(owner, block_number).await,
            Token::Native(token) => token.get_balance(owner, block_number).await,
        }
    }

    async fn get_allowance(
        &self,
        owner: Address,
        spender: Address,
        block_number: Option<u64>,
    ) -> Result<U256, ArbRsError> {
        match self {
            Token::Erc20(token) => token.get_allowance(owner, spender, block_number).await,
            Token::Native(token) => token.get_allowance(owner, spender, block_number).await,
        }
    }

    async fn get_total_supply(&self, block_number: Option<u64>) -> Result<U256, ArbRsError> {
        match self {
            Token::Erc20(token) => token.get_total_supply(block_number).await,
            Token::Native(token) => token.get_total_supply(block_number).await,
        }
    }
}

impl<P: Provider + Send + Sync + ?Sized + 'static> PartialEq for Token<P> {
    fn eq(&self, other: &Self) -> bool {
        self.address() == other.address()
    }
}

impl<P: Provider + Send + Sync + ?Sized + 'static> Eq for Token<P> {}

impl<P: Provider + Send + Sync + ?Sized + 'static> PartialOrd for Token<P> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.address().partial_cmp(&other.address())
    }
}
impl<P: Provider + Send + Sync + ?Sized + 'static> Ord for Token<P> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.address().cmp(&other.address())
    }
}

impl<P: Provider + Send + Sync + ?Sized + 'static> PartialEq<Address> for Token<P> {
    fn eq(&self, other: &Address) -> bool {
        self.address() == *other
    }
}

impl<P: Provider + Send + Sync + ?Sized + 'static> Hash for Token<P> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.address().hash(state);
    }
}

impl<P: ?Sized> Debug for Token<P> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Token::Erc20(data) => f.debug_tuple("Token::Erc20").field(&data.address).finish(),
            Token::Native(data) => f
                .debug_tuple("Token::Native")
                .field(&data.placeholder_address)
                .finish(),
        }
    }
}
