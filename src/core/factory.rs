use crate::core::contracts::{AnyError, IEvent, IEventHandler, IRequest, IRequestHandler};
use async_trait::async_trait;

#[async_trait]
pub trait RequestProvidesFactory<TRequestHandler, TRequest, TResponse, TError>
where
    TRequestHandler: IRequestHandler<TRequest, TResponse, TError>,
    TRequest: IRequest<TResponse, TError>,
    TResponse: Sized + Send + Sync + 'static,
    TError: AnyError,
{
    async fn factory() -> Result<TRequestHandler, TError>;
}

#[async_trait]
pub trait EventProvidesFactory<TEventHandler, TEvent, TError>
where
    TEventHandler: IEventHandler<TEvent, TError>,
    TEvent: IEvent<TError>,
    TError: AnyError,
{
    async fn factory() -> Result<TEventHandler, TError>;
}

#[async_trait]
pub trait RequestPipelineProvidesFactory<TPipeline, TError>
where
    TPipeline: crate::core::contracts::IRequestPipeline,
    TError: AnyError,
{
    async fn factory() -> Result<TPipeline, TError>;
}

#[async_trait]
pub trait EventPipelineProvidesFactory<TPipeline, TError>
where
    TPipeline: crate::core::contracts::IEventPipeline,
    TError: AnyError,
{
    async fn factory() -> Result<TPipeline, TError>;
}

#[cfg(any(feature = "sea-orm-postgres", feature = "sea-orm-mysql"))]
#[async_trait]
pub trait EventDatabaseProvidesFactory<TEventHandler, TEvent, TError>
where
    TEventHandler: crate::core::contracts::IEventHandlerDatabase<TEvent, TError>,
    TEvent: IEvent<TError>,
    TError: AnyError,
{
    async fn factory() -> Result<TEventHandler, TError>;
}

#[cfg(any(feature = "sea-orm-postgres", feature = "sea-orm-mysql"))]
#[async_trait]
pub trait EventDatabasePipelineProvidesFactory<TPipeline, TError>
where
    TPipeline: crate::core::contracts::IEventDatabasePipeline,
    TError: AnyError,
{
    async fn factory() -> Result<TPipeline, TError>;
}
