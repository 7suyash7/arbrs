use crate::core::token::{Erc20Data, NativeTokenData, Token};
use crate::core::token_fetcher::TokenFetcher;
use crate::errors::ArbRsError;
use crate::db::DbManager;
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
    db_manager: Arc<DbManager>,
}

impl<P: Provider + Send + Sync + 'static + ?Sized> TokenManager<P> {
    pub fn new(provider: Arc<P>, chain_id: u64, db_manager: Arc<DbManager>) -> Self {
        Self {
            chain_id,
            provider,
            token_registry: Arc::new(DashMap::new()),
            db_manager,
        }
    }

    pub async fn get_token(&self, address: Address) -> Result<Arc<Token<P>>, ArbRsError> {
        if let Some(token_entry) = self.token_registry.get(&address) {
            return Ok(token_entry.clone());
        }

        if NATIVE_PLACEHOLDERS.contains(&address) {
            let native_token = Arc::new(Token::Native(Arc::new(NativeTokenData::new(
                self.chain_id,
                address,
                self.provider.clone(),
            ))));
            self.token_registry.insert(address, native_token.clone());
            return Ok(native_token);
        }

        if let Ok(Some(record)) = self.db_manager.get_token_by_address(address).await {
            tracing::debug!(?address, symbol = record.symbol, "[CACHE HIT] Loaded token from DB.");
            let erc20_data = Erc20Data::new(
                record.address,
                record.symbol,
                "Unknown".to_string(),
                record.decimals,
                self.provider.clone(),
            );
            let token = Arc::new(Token::Erc20(Arc::new(erc20_data)));
            self.token_registry.insert(address, token.clone());
            return Ok(token);
        }

        tracing::debug!(?address, "[CACHE MISS] Fetching token from on-chain...");
        let fetcher = TokenFetcher::new(Arc::clone(&self.provider));
        let erc20_data = fetcher.fetch_erc20_data(address).await?;

        if let Err(e) = self
            .db_manager
            .save_token(&Token::Erc20(Arc::new(erc20_data.clone())))
            .await
        {
            tracing::warn!(?address, "Failed to save token to DB: {:?}", e);
        }

        let new_token = Arc::new(Token::Erc20(Arc::new(erc20_data)));
        self.token_registry.insert(address, new_token.clone());
        Ok(new_token)
    }
}

impl<P: ?Sized> Clone for Erc20Data<P> {
    fn clone(&self) -> Self {
        Self {
            address: self.address,
            symbol: self.symbol.clone(),
            name: self.name.clone(),
            decimals: self.decimals,
            provider: self.provider.clone(),
            balances: self.balances.clone(),
            total_supply_cache: self.total_supply_cache.clone(),
            allowance_cache: self.allowance_cache.clone(),
        }
    }
}
