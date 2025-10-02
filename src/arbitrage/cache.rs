use crate::arbitrage::types::Arbitrage;
use alloy_provider::Provider;
use std::fmt::{self, Debug};
use std::sync::Arc;
use tokio::sync::RwLock;

/// An in-memory, thread-safe cache to store discovered arbitrage paths.
pub struct ArbitrageCache<P: Provider + Send + Sync + 'static + ?Sized> {
    pub paths: Arc<RwLock<Vec<Arc<dyn Arbitrage<P>>>>>,
}

impl<P: Provider + Send + Sync + 'static + ?Sized> Debug for ArbitrageCache<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path_count = self.paths.try_read().map_or(0, |p| p.len());
        f.debug_struct("ArbitrageCache")
            .field("path_count", &path_count)
            .finish()
    }
}

impl<P: Provider + Send + Sync + 'static + ?Sized> ArbitrageCache<P> {
    pub fn new() -> Self {
        Self {
            paths: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn add_path(&self, path: Arc<dyn Arbitrage<P>>) {
        let mut paths = self.paths.write().await;
        paths.push(path);
    }
}

impl<P: Provider + Send + Sync + 'static + ?Sized> Default for ArbitrageCache<P> {
    fn default() -> Self {
        Self::new()
    }
}
