use crate::core::token::{NativeTokenData, Token};
use crate::core::token_fetcher::TokenFetcher;
use crate::errors::ArbRsError;
use alloy_primitives::{Address, address};
use alloy_provider::Provider;
use dashmap::DashMap;
use std::sync::Arc;

// Placeholder addresses for native currency
const NATIVE_PLACEHOLDERS: &[Address] = &[
    Address::ZERO,
    address!("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
];

pub struct TokenManager<P: ?Sized> {
    chain_id: u64,
    provider: Arc<P>,
    token_registry: Arc<DashMap<Address, Arc<Token<P>>>>,
}

impl<P: Provider + Send + Sync + 'static + ?Sized> TokenManager<P> {
    pub fn new(provider: Arc<P>, chain_id: u64) -> Self {
        Self {
            chain_id,
            provider,
            token_registry: Arc::new(DashMap::new()),
        }
    }

    pub async fn get_token(&self, address: Address) -> Result<Arc<Token<P>>, ArbRsError> {
        if let Some(token_entry) = self.token_registry.get(&address) {
            return Ok(token_entry.clone());
        }

        let new_token = if NATIVE_PLACEHOLDERS.contains(&address) {
            Arc::new(Token::Native(Arc::new(NativeTokenData::new(
                self.chain_id,
                address,
                self.provider.clone(),
            ))))
        } else {
            let fetcher = TokenFetcher::new(Arc::clone(&self.provider));
            let erc20_data = fetcher.fetch_erc20_data(address).await?;
            Arc::new(Token::Erc20(Arc::new(erc20_data)))
        };

        self.token_registry.insert(address, new_token.clone());
        Ok(new_token)
    }
}
