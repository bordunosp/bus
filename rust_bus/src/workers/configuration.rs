use crate::error::BusError;
use std::collections::HashMap;
use std::sync::OnceLock;

static BUS_QUEUE_CONFIG: OnceLock<BusQueueConfiguration> = OnceLock::new();

impl BusQueueConfiguration {
    #[allow(dead_code)]
    pub(crate) fn set_global(config: BusQueueConfiguration) -> Result<(), BusError> {
        BUS_QUEUE_CONFIG.set(config).map_err(|_| {
            BusError::Configuration(
                "BusQueueConfiguration has already been initialized".to_string(),
            )
        })
    }

    pub(crate) fn global() -> Result<&'static BusQueueConfiguration, BusError> {
        BUS_QUEUE_CONFIG.get().ok_or_else(|| {
            BusError::Configuration(
                "BusQueueConfiguration must be initialized before use. Call BusQueueConfiguration::set_global first.".to_string(),
            )
        })
    }

    #[cfg(feature = "_db_sea_orm")]
    pub(crate) fn get_connection(&self) -> &sea_orm::DatabaseConnection {
        &self.connection
    }

    #[cfg(feature = "sqlx-postgres")]
    pub(crate) fn get_connection(&self) -> &sqlx::PgPool {
        &self.connection
    }

    #[cfg(feature = "sqlx-mysql")]
    pub(crate) fn get_connection(&self) -> &sqlx::MySqlPool {
        &self.connection
    }

    pub(crate) fn get_queue(&self, name: &str) -> Result<&QueueConfiguration, BusError> {
        self.queues.get(name).ok_or_else(|| {
            BusError::Configuration(format!("Queue configuration for '{}' not found", name))
        })
    }

    pub(crate) fn queues(&self) -> &HashMap<String, QueueConfiguration> {
        &self.queues
    }

    pub(crate) fn srv_name(&self) -> &str {
        &self.srv_name
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct BusQueueConfiguration {
    srv_name: String,

    #[cfg(feature = "_db_sea_orm")]
    connection: sea_orm::DatabaseConnection,

    #[cfg(feature = "sqlx-postgres")]
    connection: sqlx::PgPool,

    #[cfg(feature = "sqlx-mysql")]
    connection: sqlx::MySqlPool,

    queues: HashMap<String, QueueConfiguration>,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct QueueConfiguration {
    pub(crate) workers: u32,
    pub(crate) max_attempts: u32,
    pub(crate) execution_timeout: chrono::Duration,
    pub(crate) empty_queue_delay: std::time::Duration,
}

pub struct BusQueueConfigurationBuilder {
    srv_name: String,

    #[cfg(feature = "_db_sea_orm")]
    connection: Option<sea_orm::ConnectOptions>,

    #[cfg(feature = "sqlx-postgres")]
    connection: Option<sqlx::Pool<sqlx::Postgres>>,

    #[cfg(feature = "sqlx-mysql")]
    connection: Option<sqlx::Pool<sqlx::MySql>>,

    pub(crate) queues: HashMap<String, QueueConfiguration>,
}

impl Default for BusQueueConfigurationBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl BusQueueConfigurationBuilder {
    pub fn new() -> Self {
        Self {
            srv_name: uuid::Uuid::now_v7().to_string(),
            #[cfg(feature = "_db_any")]
            connection: None,
            queues: HashMap::new(),
        }
    }

    pub fn srv_name<S: Into<String>>(mut self, srv_name: S) -> Self {
        self.srv_name = srv_name.into();
        self
    }

    #[cfg(feature = "_db_sea_orm")]
    pub fn connection(mut self, opts: sea_orm::ConnectOptions) -> Self {
        self.connection = Some(opts);
        self
    }

    #[cfg(feature = "_db_sqlx")]
    pub fn connection(
        mut self,
        #[cfg(feature = "sqlx-postgres")] pool: sqlx::Pool<sqlx::Postgres>,
        #[cfg(feature = "sqlx-mysql")] pool: sqlx::Pool<sqlx::MySql>,
    ) -> Self {
        self.connection = Some(pool);
        self
    }

    pub fn add_queue(mut self, name: impl Into<String>, config: QueueConfigBuilder) -> Self {
        let name_str = name.into();
        if self.queues.contains_key(&name_str) {
            panic!("Queue '{}' is already configured!", name_str);
        }
        self.queues.insert(name_str, config.build());
        self
    }

    pub async fn build(self) -> Result<BusQueueConfiguration, BusError> {
        if self.srv_name.is_empty() {
            return Err(BusError::Configuration(
                "srv_name cannot be empty".to_string(),
            ));
        }

        if self.srv_name.len() > 128 {
            return Err(BusError::Configuration(format!(
                "srv_name is too long ({} chars). Maximum is 128.",
                self.srv_name.len()
            )));
        }

        let connection_val = self.connection.ok_or_else(|| {
            BusError::Configuration(
                "Database connection must be provided. \
                Use .connection(opts) on the BusQueueConfigurationBuilder."
                    .to_string(),
            )
        })?;

        #[cfg(feature = "_db_sea_orm")]
        let connection = {
            use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

            let db = sea_orm::Database::connect(connection_val.clone())
                .await
                .map_err(|e| BusError::Configuration(format!("SeaORM connection failed: {}", e)))?;

            let backend = db.get_database_backend();

            let sql = match backend {
                DatabaseBackend::Postgres => {
                    "SELECT COUNT(*) FROM information_schema.tables WHERE table_name IN ('bus_jobs', 'bus_jobs_peers') AND table_schema = ANY(current_schemas(false))"
                }
                DatabaseBackend::MySql => {
                    "SELECT COUNT(*) FROM information_schema.tables WHERE table_name IN ('bus_jobs', 'bus_jobs_peers') AND table_schema = DATABASE()"
                }
                _ => {
                    return Err(BusError::Configuration(
                        "Unsupported database backend".to_string(),
                    ));
                }
            };

            let res = db
                .query_one_raw(Statement::from_string(backend, sql))
                .await
                .map_err(|e| BusError::Configuration(format!("Schema check failed: {}", e)))?;

            let tables_found: i64 = match res {
                Some(row) => row.try_get_by_index(0).unwrap_or(0),
                None => 0,
            };

            if tables_found < 2 {
                let ext = match backend {
                    DatabaseBackend::Postgres => "postgres.sql",
                    DatabaseBackend::MySql => "mysql.sql",
                    _ => "sql",
                };
                return Err(BusError::Configuration(format!(
                    "Required tables missing. Please run migrations from: ../../../migrations/{}",
                    ext
                )));
            }

            db
        };

        #[cfg(feature = "sqlx-postgres")]
        let connection = {
            let tables_exist: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM information_schema.tables
                     WHERE table_name IN ('bus_jobs', 'bus_jobs_peers') AND table_schema = ANY(current_schemas(false))"
            )
                .fetch_one(&connection_val)
                .await
                .map_err(|e| BusError::Configuration(e.to_string()))?;

            if tables_exist.0 < 2 {
                return Err(BusError::Configuration(
                    "Required tables 'bus_jobs' or 'bus_jobs_peers' are missing. \
                            Please run migrations from: ../../../migrations/postgres.sql"
                        .to_string(),
                ));
            }
            connection_val
        };

        #[cfg(feature = "sqlx-mysql")]
        let connection = {
            let tables_exist: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM information_schema.tables 
                     WHERE table_name IN ('bus_jobs', 'bus_jobs_peers') AND table_schema = DATABASE()"
            )
                .fetch_one(&connection_val)
                .await
                .map_err(|e| BusError::Configuration(e.to_string()))?;

            if tables_exist.0 < 2 {
                return Err(BusError::Configuration(
                    "Required tables 'bus_jobs' or 'bus_jobs_peers' are missing. \
                            Please run migrations from: ../../../migrations/mysql.sql"
                        .to_string(),
                ));
            }
            connection_val
        };

        if self.queues.is_empty() {
            return Err(BusError::Configuration(
                "At least one queue must be configured. \
                Use BusQueueConfigurationBuilder::add_queue(\"queue_name\", QueueConfigBuilder::new().build()) \
                to define your worker queues before calling init().".to_string(),
            ));
        }

        Ok(BusQueueConfiguration {
            srv_name: self.srv_name,
            connection,
            queues: self.queues,
        })
    }
}

pub struct QueueConfigBuilder {
    workers: u32,
    max_attempts: u32,
    execution_timeout: chrono::Duration,
    empty_queue_delay: std::time::Duration,
}

impl Default for QueueConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl QueueConfigBuilder {
    pub fn new() -> Self {
        Self {
            workers: 1,
            max_attempts: 3,
            execution_timeout: chrono::Duration::minutes(10),
            empty_queue_delay: std::time::Duration::from_secs(5),
        }
    }

    pub fn workers(mut self, count: u32) -> Self {
        self.workers = count;
        self
    }

    pub fn max_attempts(mut self, count: u32) -> Self {
        self.max_attempts = count;
        self
    }

    pub fn execution_timeout(mut self, duration: chrono::Duration) -> Self {
        self.execution_timeout = duration;
        self
    }

    pub fn empty_queue_delay(mut self, delay: std::time::Duration) -> Self {
        self.empty_queue_delay = delay;
        self
    }

    fn build(self) -> QueueConfiguration {
        QueueConfiguration {
            workers: self.workers,
            max_attempts: self.max_attempts,
            execution_timeout: self.execution_timeout,
            empty_queue_delay: self.empty_queue_delay,
        }
    }
}
