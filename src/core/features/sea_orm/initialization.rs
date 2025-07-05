use crate::core::error_bus::BusError;
use crate::core::features::sea_orm::core::DatabaseQueueProcessorConfiguration;
use once_cell::sync::OnceCell;
use sea_orm::{Database, DatabaseConnection};

static DB_CONN: OnceCell<DatabaseConnection> = OnceCell::new();
static QUEUE_CONFIG: OnceCell<DatabaseQueueProcessorConfiguration> = OnceCell::new();

pub async fn init_sea_orm(
    queue_configuration: DatabaseQueueProcessorConfiguration,
) -> Result<(), BusError> {
    let conn = Database::connect(
        queue_configuration
            .connection()
            .sea_orm_connect_options()
            .clone(),
    )
    .await
    .map_err(BusError::DbErr)?;

    DB_CONN
        .set(conn)
        .map_err(|_| BusError::AlreadyInitialized("queue_configuration.connection".into()))?;

    QUEUE_CONFIG
        .set(queue_configuration)
        .map_err(|_| BusError::AlreadyInitialized("queue_configuration.queues".into()))?;

    Ok(())
}

pub(crate) fn get_db_conn() -> Result<&'static DatabaseConnection, BusError> {
    DB_CONN
        .get()
        .ok_or(BusError::NotInitialized("db: DbConn".into()))
}

pub(crate) fn get_queue_config() -> Result<&'static DatabaseQueueProcessorConfiguration, BusError> {
    QUEUE_CONFIG.get().ok_or(BusError::NotInitialized(
        "queue_configuration.queues".into(),
    ))
}

#[cfg(test)]
pub fn init_sea_orm_for_tests(
    conn: DatabaseConnection,
    config: DatabaseQueueProcessorConfiguration,
) -> Result<(), BusError> {
    if DB_CONN.get().is_none() {
        DB_CONN
            .set(conn)
            .map_err(|_| BusError::AlreadyInitialized("queue_configuration.connection".into()))?;
    }

    if QUEUE_CONFIG.get().is_none() {
        QUEUE_CONFIG
            .set(config)
            .map_err(|_| BusError::AlreadyInitialized("queue_configuration.queues".into()))?;
    }

    Ok(())
}
