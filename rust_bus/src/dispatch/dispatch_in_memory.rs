use crate::contracts::bus_event::IBusEvent;
use crate::dispatch::registration::MEMORY_HANDLERS;
use crate::error::BusError;
use futures::FutureExt;
use std::panic::AssertUnwindSafe;

#[cfg(not(feature = "context"))]
pub(crate) async fn dispatch_in_memory<'a, TEvent>(
    #[cfg(feature = "_db_any")]
    #[cfg(feature = "sqlx-postgres")]
    txn: &mut sqlx::Transaction<'a, sqlx::Postgres>,
    #[cfg(feature = "sqlx-mysql")] txn: &mut sqlx::Transaction<'a, sqlx::MySql>,
    #[cfg(feature = "_db_sea_orm")] txn: &'a sea_orm::DatabaseTransaction,
    event: &TEvent,
    metadata: &crate::contracts::meta::BusMetadata,
) -> Result<usize, Box<dyn std::error::Error + Send + Sync>>
where
    TEvent: IBusEvent,
{
    let event_identity = TEvent::EVENT_IDENTITY;
    let event_ptr = event as *const TEvent as usize;
    let meta_ptr = metadata as *const crate::contracts::meta::BusMetadata as usize;

    #[cfg(feature = "_db_any")]
    let txn_ptr = {
        #[cfg(feature = "_db_sqlx")]
        {
            txn as *mut _ as usize
        }

        #[cfg(feature = "_db_sea_orm")]
        {
            txn as *const _ as usize
        }
    };

    #[cfg(feature = "logging")]
    log::trace!(
        "Dispatching in-memory event (no context): {}",
        event_identity
    );

    let handlers_map = MEMORY_HANDLERS.get().ok_or_else(|| {
        BusError::Configuration(
            "Bus not initialized. Call Bus::init() before dispatching.".to_string(),
        )
    })?;

    let handlers = match handlers_map.get(event_identity) {
        None => return Ok(0),
        Some(h) => h,
    };

    for reg in handlers.iter() {
        #[cfg(feature = "logging")]
        log::trace!(
            "Executing handler: {} for event: {}",
            reg.handler_identity,
            event_identity
        );

        #[cfg(feature = "_db_any")]
        let res = AssertUnwindSafe((reg.execute)(txn_ptr, event_ptr, meta_ptr))
            .catch_unwind()
            .await;
        #[cfg(not(feature = "_db_any"))]
        let res = AssertUnwindSafe((reg.execute)(event_ptr, meta_ptr))
            .catch_unwind()
            .await;

        handle_execution_result(reg.handler_identity, event_identity, res)?;
    }

    Ok(handlers.len())
}

#[cfg(feature = "context")]
pub(crate) async fn dispatch_in_memory<TContext, TEvent>(
    ctx: TContext,
    event: &TEvent,
) -> Result<usize, Box<dyn std::error::Error + Send + Sync>>
where
    TEvent: IBusEvent,
    TContext: crate::contracts::ctx::ToRawContext,
{
    let event_identity = TEvent::EVENT_IDENTITY;
    let event_ptr = event as *const TEvent as usize;

    #[cfg(feature = "logging")]
    log::trace!(
        "Dispatching in-memory event: {} with context (mut: {})",
        event_identity,
        ctx.to_raw().is_mutable
    );

    let handlers_map = MEMORY_HANDLERS.get().ok_or_else(|| {
        BusError::Configuration(
            "Bus not initialized. Call Bus::init() before dispatching.".to_string(),
        )
    })?;

    let handlers = match handlers_map.get(event_identity) {
        None => return Ok(0),
        Some(h) => h,
    };

    for reg in handlers.iter() {
        #[cfg(feature = "logging")]
        log::trace!(
            "Executing handler: {} for event: {}",
            reg.handler_identity,
            event_identity
        );

        let res = AssertUnwindSafe((reg.execute)(ctx.to_raw(), event_ptr))
            .catch_unwind()
            .await;
        handle_execution_result(reg.handler_identity, event_identity, res)?;
    }

    Ok(handlers.len())
}

fn handle_execution_result(
    _handler_id: &str,
    _event_id: &str,
    result: Result<
        Result<(), Box<dyn std::error::Error + Send + Sync>>,
        Box<dyn std::any::Any + Send>,
    >,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    match result {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => {
            #[cfg(feature = "logging")]
            log::error!(
                "Handler {} failed for event {}: {:?}",
                _handler_id,
                _event_id,
                e
            );
            Err(e)
        }
        Err(panic_payload) => {
            #[cfg(feature = "logging")]
            log::error!("Handler {} PANICKED for event {}", _handler_id, _event_id);

            let msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                s.to_string()
            } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                s.clone()
            } else {
                "Unknown panic message".to_string()
            };
            Err(format!("Handler panicked: {}", msg).into())
        }
    }
}
