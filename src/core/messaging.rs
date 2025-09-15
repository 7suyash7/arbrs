use crate::pool::uniswap_v2::UniswapV2PoolState;
use alloy_provider::Provider;
use async_trait::async_trait;
use std::sync::Weak;

/// A message sent by a `Publisher` to a `Subscriber`.
#[derive(Debug, Clone)]
pub enum PublisherMessage {
    PoolStateUpdate(UniswapV2PoolState),
    // You can add other message types here later
}

/// A trait for objects that can be subscribed to.
#[async_trait]
pub trait Publisher<P: Provider + Send + Sync + 'static + ?Sized>: Send + Sync {
    async fn subscribe(&self, subscriber: Weak<dyn Subscriber<P>>);
    async fn unsubscribe(&self, subscriber_id: usize);
    async fn notify_subscribers(&self, message: PublisherMessage);
}

/// A trait for objects that can receive notifications.
#[async_trait]
pub trait Subscriber<P: Provider + Send + Sync + 'static + ?Sized>: Send + Sync {
    fn id(&self) -> usize;
    async fn notify(&self, message: PublisherMessage);
}