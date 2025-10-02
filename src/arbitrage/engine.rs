use crate::arbitrage::cache::ArbitrageCache;
use alloy_primitives::U256;
use alloy_provider::Provider;
use futures::future::join_all;
use std::fmt::{self, Debug};
use std::sync::Arc;
use tokio::task::JoinHandle;

/// The main engine responsible for evaluating arbitrage opportunities.
pub struct ArbitrageEngine<P: Provider + Send + Sync + 'static + ?Sized> {
    pub cache: Arc<ArbitrageCache<P>>,
}

impl<P: Provider + Send + Sync + 'static + ?Sized> ArbitrageEngine<P> {
    pub fn new(cache: Arc<ArbitrageCache<P>>) -> Self {
        Self { cache }
    }

    pub async fn calculate_all_paths(&self, block_number: Option<u64>) {
        let paths = self.cache.paths.read().await;
        let mut tasks: Vec<JoinHandle<_>> = Vec::new();

        println!("Calculating profit for {} paths...", paths.len());

        for (i, path) in paths.iter().enumerate() {
            let path_clone = path.clone();

            let task = tokio::spawn(async move {
                let start_amount = U256::from(10).pow(U256::from(18));

                match path_clone
                    .calculate_profit(start_amount, block_number)
                    .await
                {
                    Ok((profit, _amount_out)) => {
                        if profit > U256::ZERO {
                            println!(
                                "[ profitable opportunity! ] Path #{}: Profit: {}",
                                i, profit,
                            );
                        }
                    }
                    Err(_e) => {
                        // need to add error handling here
                    }
                }
            });

            tasks.push(task);
        }

        join_all(tasks).await;
    }
}

impl<P: Provider + Send + Sync + 'static + ?Sized> Debug for ArbitrageEngine<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ArbitrageEngine")
            .field("cache", &self.cache)
            .finish()
    }
}

impl<P: Provider + Send + Sync + 'static + ?Sized> Clone for ArbitrageEngine<P> {
    fn clone(&self) -> Self {
        Self {
            cache: self.cache.clone(),
        }
    }
}