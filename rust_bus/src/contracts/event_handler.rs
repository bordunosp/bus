use crate::contracts::bus_event::IBusEvent;

#[cfg(all(not(feature = "context"), feature = "_db_any"))]
pub trait IEventHandler<'a, TEvent>: Default + Send + Sync
where
    TEvent: IBusEvent,
{
    const HANDLER_IDENTITY: &'static str;

    #[cfg(feature = "sqlx-postgres")]
    fn handle(
        &self,
        txn: &mut sqlx::Transaction<'a, sqlx::Postgres>,
        event: &TEvent,
        metadata: &crate::contracts::meta::BusMetadata,
    ) -> impl Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send;

    #[cfg(feature = "sqlx-mysql")]
    fn handle(
        &self,
        txn: &mut sqlx::Transaction<'a, sqlx::MySql>,
        event: &TEvent,
        metadata: &crate::contracts::meta::BusMetadata,
    ) -> impl Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send;

    #[cfg(feature = "_db_sea_orm")]
    fn handle(
        &self,
        txn: &'a sea_orm::DatabaseTransaction,
        event: &TEvent,
        metadata: &crate::contracts::meta::BusMetadata,
    ) -> impl Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send;
}

#[cfg(all(not(feature = "context"), not(feature = "_db_any")))]
pub trait IEventHandler<TEvent>: Default + Send + Sync
where
    TEvent: IBusEvent,
{
    const HANDLER_IDENTITY: &'static str;

    fn handle(
        &self,
        event: &TEvent,
        metadata: &crate::contracts::meta::BusMetadata,
    ) -> impl Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send;
}

#[cfg(all(feature = "context", feature = "_db_any"))]
pub trait IEventHandler<'a, TContext, TEvent>: Default + Send + Sync
where
    TEvent: IBusEvent,
    TContext: crate::contracts::ctx::IBusContext<'a> + ?Sized,
{
    const HANDLER_IDENTITY: &'static str;

    fn handle(
        &self,
        ctx: TContext,
        event: &TEvent,
    ) -> impl Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send;
}

#[cfg(all(feature = "context", not(feature = "_db_any")))]
pub trait IEventHandler<TContext, TEvent>: Default + Send + Sync
where
    TEvent: IBusEvent,
    TContext: crate::contracts::ctx::IBusContext + ?Sized,
{
    const HANDLER_IDENTITY: &'static str;

    fn handle(
        &self,
        ctx: TContext,
        event: &TEvent,
    ) -> impl Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send;
}
