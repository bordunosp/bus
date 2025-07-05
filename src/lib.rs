pub mod core;
pub mod registry;

use crate::core::contracts::{AnyError, IEvent, IRequest};
use crate::core::event_handlers;
use crate::core::request_handlers;

#[cfg(all(feature = "sea-orm-postgres", feature = "sea-orm-mysql"))]
compile_error!(
    "Cannot enable both 'sea-orm-postgres' and 'sea-orm-mysql' features simultaneously. Please choose one."
);

#[cfg(any(feature = "sea-orm-postgres", feature = "sea-orm-mysql"))]
use {
    crate::core::features::sea_orm::event_handlers_database::insert_pending_bus_events,
    crate::core::features::sea_orm::event_handlers_database::insert_pending_bus_events_txn,
    sea_orm::DatabaseTransaction,
};

pub async fn send<TRequest, TResponse, TError>(
    request: TRequest,
    #[cfg(feature = "cancellation-token")] cancellation_token: tokio_util::sync::CancellationToken,
) -> Result<TResponse, TError>
where
    TRequest: IRequest<TResponse, TError>,
    TResponse: Sized + Send + Sync + 'static,
    TError: AnyError,
{
    request_handlers::handle(
        request,
        #[cfg(feature = "cancellation-token")]
        cancellation_token,
    )
    .await
}

pub async fn publish<TEvent, TError>(
    event: TEvent,
    #[cfg(feature = "cancellation-token")] cancellation_token: tokio_util::sync::CancellationToken,
) -> Result<(), TError>
where
    TEvent: IEvent<TError>,
    TError: AnyError,
{
    event_handlers::publish(
        event.clone(),
        #[cfg(feature = "cancellation-token")]
        cancellation_token,
    )
    .await?;

    #[cfg(any(feature = "sea-orm-postgres", feature = "sea-orm-mysql"))]
    insert_pending_bus_events(event.clone()).await?;
    Ok(())
}

#[cfg(any(feature = "sea-orm-postgres", feature = "sea-orm-mysql"))]
pub async fn publish_txn<TEvent, TError, TConnection>(
    txn: DatabaseTransaction,
    event: TEvent,
    #[cfg(feature = "cancellation-token")] cancellation_token: tokio_util::sync::CancellationToken,
) -> Result<(), TError>
where
    TEvent: IEvent<TError>,
    TError: AnyError,
{
    event_handlers::publish(
        event.clone(),
        #[cfg(feature = "cancellation-token")]
        cancellation_token,
    )
    .await?;

    insert_pending_bus_events_txn(Some(txn), event.clone()).await?;
    Ok(())
}
