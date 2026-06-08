use once_cell::sync::OnceCell;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

#[cfg(feature = "_db_any")]
use crate::contracts::database_unique::Unique;
#[cfg(feature = "_db_any")]
use crate::contracts::enums::ScheduleIn;

pub type RawPtr = usize;

pub type BoxedBusFuture = Pin<
    Box<dyn Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send + 'static>,
>;

#[allow(dead_code)]
#[cfg(feature = "_db_any")]
pub type DatabaseHandlerFn =
    fn(db: RawPtr, event: &serde_json::Value, meta: RawPtr) -> BoxedBusFuture;

#[cfg(feature = "context")]
pub mod types {
    use crate::contracts::ctx::RawContext;
    use crate::dispatch::registration::{BoxedBusFuture, RawPtr};

    pub type MemoryHandlerFn = fn(ctx: RawContext, event: RawPtr) -> BoxedBusFuture;
}

#[cfg(not(feature = "context"))]
pub mod types {
    use crate::dispatch::registration::{BoxedBusFuture, RawPtr};

    #[cfg(feature = "_db_any")]
    pub type MemoryHandlerFn = fn(txn: RawPtr, event: RawPtr, meta: RawPtr) -> BoxedBusFuture;

    #[cfg(not(feature = "_db_any"))]
    pub type MemoryHandlerFn = fn(event: RawPtr, meta: RawPtr) -> BoxedBusFuture;
}

pub use types::*;

#[allow(dead_code)]
pub struct EventHandlerRegistration {
    pub handler_identity: &'static str,
    pub event_identity: &'static str,
    #[cfg(feature = "context")]
    pub context_identity: &'static str,
    pub execute: MemoryHandlerFn,
}
inventory::collect!(EventHandlerRegistration);

#[allow(dead_code)]
#[cfg(feature = "_db_any")]
pub struct EventDatabaseHandlerRegistration {
    pub handler_identity: &'static str,
    pub event_identity: &'static str,
    pub queue: &'static str,
    pub priority: u32,
    pub max_attempts: Option<u32>,
    pub execution_timeout: Option<chrono::Duration>,
    pub tags: &'static [&'static str],
    pub unique: Option<Unique>,
    pub schedule_in: ScheduleIn,
    pub next_attempt_at: fn(u32) -> chrono::Duration,
    pub execute: DatabaseHandlerFn,
}

#[cfg(feature = "_db_any")]
inventory::collect!(EventDatabaseHandlerRegistration);

pub static MEMORY_HANDLERS: OnceCell<
    HashMap<&'static str, Vec<&'static EventHandlerRegistration>>,
> = OnceCell::new();

#[cfg(feature = "_db_any")]
pub static DATABASE_HANDLERS: OnceCell<
    HashMap<&'static str, Vec<&'static EventDatabaseHandlerRegistration>>,
> = OnceCell::new();

#[cfg(feature = "_db_any")]
pub static DATABASE_HANDLERS_BY_HASH: OnceCell<
    HashMap<i64, &'static EventDatabaseHandlerRegistration>,
> = OnceCell::new();
