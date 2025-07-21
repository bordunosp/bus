use crate::core::error_bus::BusError;
use async_trait::async_trait;
use std::any::{Any, type_name};
use std::error::Error;
use std::sync::Arc;

pub trait TypeNamed {
    fn type_name(&self) -> &'static str {
        type_name::<Self>()
    }
}

impl<T> TypeNamed for T {
    fn type_name(&self) -> &'static str {
        type_name::<T>()
    }
}

pub trait AnyError: Error + From<BusError> + Send + Sync + 'static {}
impl<T> AnyError for T where T: Error + From<BusError> + Send + Sync + 'static {}

pub trait IRequest<TResponse, TError>: TypeNamed + Sized + Send + Sync + 'static
where
    TResponse: Sized + Send + Sync + 'static,
    TError: AnyError,
{
}

pub trait IEvent<TError>:
    serde::Serialize + serde::de::DeserializeOwned + TypeNamed + Clone + Send + Sync + 'static
where
    TError: AnyError,
{
}

#[async_trait]
pub trait IRequestHandler<TRequest, TResponse, TError>: Send + Sync + 'static
where
    TRequest: IRequest<TResponse, TError>,
    TResponse: Sized + Send + Sync + 'static,
    TError: AnyError,
{
    async fn handle_async(
        &self,
        request: TRequest,
        #[cfg(feature = "cancellation-token")]
        cancellation_token: tokio_util::sync::CancellationToken,
    ) -> Result<TResponse, TError>;
}

#[async_trait]
pub trait IErasedRequestHandler: Send + Sync {
    async fn handle(
        &self,
        request: Box<dyn Any + Send + Sync>,
        #[cfg(feature = "cancellation-token")]
        cancellation_token: tokio_util::sync::CancellationToken,
    ) -> Result<Box<dyn Any + Send + Sync>, Box<dyn Error>>;
}

#[async_trait]
pub trait IRequestPipeline: Send + Sync {
    async fn handle(
        &self,
        request: Box<dyn Any + Send + Sync>,
        next: Arc<dyn IErasedRequestHandler>,
        #[cfg(feature = "cancellation-token")]
        cancellation_token: tokio_util::sync::CancellationToken,
    ) -> Result<Box<dyn Any + Send + Sync>, Box<dyn Error>>;
}

#[async_trait]
pub trait IErasedEventHandler: Send + Sync {
    async fn handle(
        &self,
        event: Box<dyn Any + Send + Sync>,
        #[cfg(feature = "cancellation-token")]
        cancellation_token: tokio_util::sync::CancellationToken,
    ) -> Result<(), Box<dyn Error>>;
}

#[async_trait]
pub trait IEventPipeline: Send + Sync {
    async fn handle(
        &self,
        event: Box<dyn Any + Send + Sync>,
        next: Arc<dyn IErasedEventHandler>,
        #[cfg(feature = "cancellation-token")]
        cancellation_token: tokio_util::sync::CancellationToken,
    ) -> Result<(), Box<dyn Error>>;
}

#[async_trait]
pub trait IEventHandler<TEvent, TError>: Send + Sync + 'static
where
    TEvent: IEvent<TError>,
    TError: AnyError,
{
    async fn handle_async(
        &self,
        event: TEvent,
        #[cfg(feature = "cancellation-token")]
        cancellation_token: tokio_util::sync::CancellationToken,
    ) -> Result<(), TError>;
}

#[cfg(any(feature = "sea-orm-postgres", feature = "sea-orm-mysql"))]
pub struct EventQueueSettings {
    pub queue_name: String,
    pub max_retries: i32,
    pub execution_timeout: chrono::Duration,
}

#[cfg(any(feature = "sea-orm-postgres", feature = "sea-orm-mysql"))]
#[async_trait]
pub trait IEventHandlerDatabase<TEvent, TError>: Send + Sync + 'static
where
    TEvent: IEvent<TError>,
    TError: AnyError,
{
    fn settings() -> EventQueueSettings;
    fn calculate_scheduled_at(current_retry: i32) -> chrono::Duration;

    async fn handle_async(&self, event: TEvent) -> Result<(), TError>;
}

#[cfg(any(feature = "sea-orm-postgres", feature = "sea-orm-mysql"))]
#[async_trait]
pub trait IEventDatabasePipeline: Send + Sync {
    async fn handle(
        &self,
        request: Box<dyn Any + Send + Sync>,
        next: Arc<dyn IErasedEventHandlerDatabase>,
    ) -> Result<(), Box<dyn Error + Send + Sync>>;
}

#[cfg(any(feature = "sea-orm-postgres", feature = "sea-orm-mysql"))]
#[async_trait]
pub trait IErasedEventHandlerDatabase: Send + Sync {
    async fn handle(
        &self,
        event: Box<dyn Any + Send + Sync>,
    ) -> Result<(), Box<dyn Error + Send + Sync>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MyStruct;

    #[test]
    fn test_type_named_returns_correct_type_name() {
        let instance = MyStruct;
        let type_name = instance.type_name();
        assert_eq!(type_name, std::any::type_name::<MyStruct>());
    }
}
