use crate::contracts::bus_event::IBusEvent;
use crate::dispatch::dispatch_in_memory::dispatch_in_memory;

#[cfg(all(not(feature = "context"), not(feature = "_db_any")))]
pub async fn event<TEvent>(
    event: TEvent,
    metadata: &crate::contracts::meta::BusMetadata,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    TEvent: IBusEvent,
{
    let _cnt = dispatch_in_memory(&event, metadata).await?;

    #[cfg(feature = "logging")]
    {
        if _cnt == 0 {
            log::warn!("No handlers found for event: {}", TEvent::EVENT_IDENTITY);
        } else {
            log::trace!(
                "Successfully dispatched handlers: {} to {} event",
                _cnt,
                TEvent::EVENT_IDENTITY,
            );
        }
    }
    Ok(())
}

#[cfg(all(not(feature = "context"), feature = "_db_any"))]
pub async fn event<'a, TEvent>(
    #[cfg(feature = "sqlx-postgres")] txn: &mut sqlx::Transaction<'a, sqlx::Postgres>,
    #[cfg(feature = "sqlx-mysql")] txn: &mut sqlx::Transaction<'a, sqlx::MySql>,
    #[cfg(feature = "_db_sea_orm")] txn: &'a sea_orm::DatabaseTransaction,
    event: TEvent,
    metadata: &crate::contracts::meta::BusMetadata,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    TEvent: IBusEvent,
{
    let mut _cnt = dispatch_in_memory(txn, &event, metadata).await?;
    _cnt += crate::dispatch::dispatch_db::dispatch_db(txn, &event, metadata).await?;

    #[cfg(feature = "logging")]
    {
        if _cnt == 0 {
            log::warn!("No handlers found for event: {}", TEvent::EVENT_IDENTITY);
        } else {
            log::trace!(
                "Successfully dispatched handlers: {} to {} event",
                _cnt,
                TEvent::EVENT_IDENTITY,
            );
        }
    }
    Ok(())
}

#[cfg(all(feature = "context", feature = "_db_sqlx"))]
pub async fn event<'a, TContext, TEvent>(
    ctx: &mut TContext,
    event: TEvent,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    TEvent: IBusEvent,
    TContext: crate::contracts::ctx::IBusContext<'a> + ?Sized,
{
    let mut _cnt = dispatch_in_memory(&mut *ctx, &event).await?;

    let metadata = ctx.metadata().clone();
    _cnt += crate::dispatch::dispatch_db::dispatch_db(ctx.txn(), &event, &metadata).await?;

    #[cfg(feature = "logging")]
    {
        if _cnt == 0 {
            log::warn!("No handlers found for event: {}", TEvent::EVENT_IDENTITY);
        } else {
            log::trace!(
                "Successfully dispatched handlers: {} to {} event",
                _cnt,
                TEvent::EVENT_IDENTITY,
            );
        }
    }
    Ok(())
}

#[cfg(all(feature = "context", feature = "_db_sea_orm"))]
pub async fn event<'a, TContext, TEvent>(
    ctx: TContext,
    event: TEvent,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    TEvent: IBusEvent,
    TContext: crate::contracts::ctx::ToRawContext + crate::contracts::ctx::IBusContext<'a>,
{
    let metadata = ctx.metadata().clone();
    let txn = ctx.txn();

    let mut _cnt = dispatch_in_memory(ctx, &event).await?;

    _cnt += crate::dispatch::dispatch_db::dispatch_db(txn, &event, &metadata).await?;

    #[cfg(feature = "logging")]
    {
        if _cnt == 0 {
            log::warn!("No handlers found for event: {}", TEvent::EVENT_IDENTITY);
        } else {
            log::trace!(
                "Successfully dispatched handlers: {} to {} event",
                _cnt,
                TEvent::EVENT_IDENTITY,
            );
        }
    }
    Ok(())
}

#[cfg(all(feature = "context", not(feature = "_db_any")))]
pub async fn event<TContext, TEvent>(
    ctx: TContext,
    event: TEvent,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    TEvent: IBusEvent,
    TContext: crate::contracts::ctx::ToRawContext,
{
    let _cnt = dispatch_in_memory(ctx, &event).await?;

    #[cfg(feature = "logging")]
    {
        if _cnt == 0 {
            log::warn!("No handlers found for event: {}", TEvent::EVENT_IDENTITY);
        } else {
            log::trace!(
                "Successfully dispatched handlers: {} to {} event",
                _cnt,
                TEvent::EVENT_IDENTITY,
            );
        }
    }
    Ok(())
}
