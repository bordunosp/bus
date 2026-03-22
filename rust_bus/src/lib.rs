#[cfg(any(
    all(feature = "_db_sea_orm", feature = "_db_sqlx"),
    all(feature = "_db_postgres", feature = "_db_mysql"),
))]
compile_error!("Multiple database backends enabled. Please select exactly one.");

#[cfg(test)]
mod tests;

pub mod bus;
pub mod contracts;
mod dispatch;
pub mod error;
mod initialization;
pub(crate) mod sql;

#[cfg(feature = "_db_sea_orm")]
pub mod models;

#[cfg(feature = "_db_any")]
#[allow(dead_code)]
pub mod workers;

pub use crate::error::BusError;
pub use inventory;
pub use rust_bus_macros::BusEvent;

#[cfg(feature = "context")]
pub use contracts::ctx::ExampleBusContext;

// +In Memory

#[cfg(feature = "context")]
pub use rust_bus_macros::{BusEventHandlerContext, BusEventHandlerContext as BusEventHandler};

#[cfg(all(not(feature = "context"), not(feature = "_db_any")))]
pub use rust_bus_macros::BusEventHandler;

#[cfg(all(not(feature = "context"), feature = "_db_sea_orm"))]
pub use rust_bus_macros::{BusEventHandlerSea, BusEventHandlerSea as BusEventHandler};

#[cfg(all(not(feature = "context"), feature = "sqlx-postgres"))]
pub use rust_bus_macros::{BusEventHandlerSqlxPg, BusEventHandlerSqlxPg as BusEventHandler};

#[cfg(all(not(feature = "context"), feature = "sqlx-mysql"))]
pub use rust_bus_macros::{BusEventHandlerSqlxMysql, BusEventHandlerSqlxMysql as BusEventHandler};

// +Database

#[cfg(feature = "_db_sea_orm")]
pub use rust_bus_macros::{
    BusEventHandlerDatabaseSea, BusEventHandlerDatabaseSea as BusEventHandlerDatabase,
};

#[cfg(feature = "sqlx-postgres")]
pub use rust_bus_macros::{
    BusEventHandlerDatabaseSqlxPg, BusEventHandlerDatabaseSqlxPg as BusEventHandlerDatabase,
};

#[cfg(feature = "sqlx-mysql")]
pub use rust_bus_macros::{
    BusEventHandlerDatabaseSqlxMysql, BusEventHandlerDatabaseSqlxMysql as BusEventHandlerDatabase,
};

pub async fn init(
    #[cfg(feature = "_db_any")]
    queue_configuration: workers::configuration::BusQueueConfigurationBuilder,
) -> Result<(), BusError> {
    #[cfg(feature = "logging")]
    log::info!("Initializing Rust Bus...");

    let (_mem_events, _mem_handlers) = initialization::init_memory::init_memory_handlers()?;

    #[cfg(feature = "_db_any")]
    {
        let (_db_events, _db_handlers) =
            initialization::init_db::init_database_handlers(queue_configuration).await?;

        #[cfg(feature = "logging")]
        log::info!(
            "Rust Bus initialized: Memory [E: {}, H: {}], DB [E: {}, H: {}].",
            _mem_events,
            _mem_handlers,
            _db_events,
            _db_handlers
        );
    }

    #[cfg(all(not(feature = "_db_any"), feature = "logging"))]
    log::info!(
        "Rust Bus initialized: Memory [E: {}, H: {}]. DB disabled.",
        _mem_events,
        _mem_handlers
    );

    Ok(())
}
