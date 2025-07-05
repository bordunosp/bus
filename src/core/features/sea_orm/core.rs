use crate::core::error_bus::BusError;
use crate::core::features::sea_orm::dto::{DatabaseConnectionDto, DatabaseQueueConfigurationDto};
use sea_orm::{ConnectOptions, DbErr, Iden};
use std::fmt;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq)]
pub enum ArchiveType {
    None,
    All,
    Completed,
    Failed,
}

impl fmt::Display for ArchiveType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArchiveType::None => write!(f, "none"),
            ArchiveType::All => write!(f, "all"),
            ArchiveType::Completed => write!(f, "completed"),
            ArchiveType::Failed => write!(f, "failed"),
        }
    }
}

impl TryFrom<&str> for ArchiveType {
    type Error = BusError;

    fn try_from(value: &str) -> Result<Self, BusError> {
        match value.trim().to_lowercase().as_str() {
            "none" => Ok(ArchiveType::None),
            "all" => Ok(ArchiveType::All),
            "completed" => Ok(ArchiveType::Completed),
            "failed" => Ok(ArchiveType::Failed),
            _ => Err(BusError::DbErr(DbErr::Custom(
                "Unknown archive type".into(),
            ))),
        }
    }
}

impl TryFrom<String> for ArchiveType {
    type Error = BusError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_from(value.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct DatabaseConnection {
    sea_orm_connect_options: ConnectOptions,
    table_name: String,
    table_name_archive: String,
}

impl DatabaseConnection {
    pub fn new(dto: DatabaseConnectionDto) -> Result<Self, BusError> {
        if !dto
            .table_name
            .clone()
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Err(BusError::InvalidTableName(dto.table_name));
        }

        if !dto
            .table_name_archive
            .clone()
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Err(BusError::InvalidTableName(dto.table_name_archive));
        }

        Ok(Self {
            sea_orm_connect_options: dto.sea_orm_connect_options.to_owned(),
            table_name: dto.table_name,
            table_name_archive: dto.table_name_archive,
        })
    }

    pub fn sea_orm_connect_options(&self) -> &ConnectOptions {
        &self.sea_orm_connect_options
    }

    pub fn table_name(&self) -> &str {
        &self.table_name
    }

    pub fn table_name_archive(&self) -> &str {
        &self.table_name_archive
    }
}

#[derive(Clone, Debug)]
pub struct DatabaseQueueConfiguration {
    queue_name: String,
    workers: usize,
    batch_size: i32,
    sleep_interval: Duration,
    archive_type: ArchiveType,
}

impl DatabaseQueueConfiguration {
    pub fn new(dto: DatabaseQueueConfigurationDto) -> Result<Self, BusError> {
        if dto.batch_size < 1 || dto.workers < 1 {
            return Err(BusError::InvalidDatabaseQueueConfiguration(
                "batch_size and workers must be >= 1".to_string(),
            ));
        }
        Ok(Self {
            queue_name: dto.queue_name,
            workers: dto.workers,
            batch_size: dto.batch_size,
            sleep_interval: dto.sleep_interval,
            archive_type: dto.archive_type,
        })
    }

    pub fn queue_name(&self) -> &str {
        &self.queue_name
    }

    pub fn workers(&self) -> usize {
        self.workers
    }

    pub fn batch_size(&self) -> i32 {
        self.batch_size
    }

    pub fn sleep_interval(&self) -> Duration {
        self.sleep_interval
    }

    pub fn archive_type(&self) -> &ArchiveType {
        &self.archive_type
    }

    pub fn should_archive_failed(&self) -> bool {
        matches!(self.archive_type, ArchiveType::All | ArchiveType::Failed)
    }

    pub fn should_archive_success(&self) -> bool {
        matches!(self.archive_type, ArchiveType::All | ArchiveType::Completed)
    }
}

#[derive(Debug, Clone)]
pub struct DatabaseQueueProcessorConfiguration {
    connection: DatabaseConnection,
    queues: Vec<DatabaseQueueConfiguration>,
}

impl DatabaseQueueProcessorConfiguration {
    pub fn new(connection: DatabaseConnection, queues: Vec<DatabaseQueueConfiguration>) -> Self {
        Self { connection, queues }
    }

    pub fn connection(&self) -> &DatabaseConnection {
        &self.connection
    }

    pub fn queues(&self) -> &Vec<DatabaseQueueConfiguration> {
        &self.queues
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::ConnectOptions;
    use std::time::Duration;

    #[test]
    fn test_archive_type_display() {
        assert_eq!(ArchiveType::None.to_string(), "none");
        assert_eq!(ArchiveType::All.to_string(), "all");
        assert_eq!(ArchiveType::Completed.to_string(), "completed");
        assert_eq!(ArchiveType::Failed.to_string(), "failed");
    }

    #[test]
    fn test_archive_type_try_from_valid() {
        assert_eq!(ArchiveType::try_from("none").unwrap(), ArchiveType::None);
        assert_eq!(ArchiveType::try_from("ALL").unwrap(), ArchiveType::All);
        assert_eq!(
            ArchiveType::try_from(" completed ").unwrap(),
            ArchiveType::Completed
        );
        assert_eq!(
            ArchiveType::try_from("failed").unwrap(),
            ArchiveType::Failed
        );
    }

    #[test]
    fn test_archive_type_try_from_invalid() {
        let err = ArchiveType::try_from("unknown").unwrap_err();
        assert!(matches!(err, BusError::DbErr(_)));
        assert!(err.to_string().contains("Unknown archive type"));
    }

    #[test]
    fn test_database_connection_valid_names() {
        let options = ConnectOptions::new("sqlite::memory:");
        let conn = DatabaseConnection::new(DatabaseConnectionDto {
            sea_orm_connect_options: options,
            table_name: "valid_table".to_string(),
            table_name_archive: "archive_table".to_string(),
        })
        .unwrap();

        assert_eq!(conn.table_name(), "valid_table");
        assert_eq!(conn.table_name_archive(), "archive_table");
    }

    #[test]
    fn test_database_connection_invalid_table_name() {
        let options = ConnectOptions::new("sqlite::memory:");

        let err = DatabaseConnection::new(DatabaseConnectionDto {
            sea_orm_connect_options: options.clone(),
            table_name: "&%&%&%&%&&%##".to_string(),
            table_name_archive: "bus_events_archive".to_string(),
        })
        .unwrap_err();

        assert!(matches!(err, BusError::InvalidTableName(_)));
    }

    #[test]
    fn test_database_connection_invalid_archive_name() {
        let options = ConnectOptions::new("sqlite::memory:");

        let err = DatabaseConnection::new(DatabaseConnectionDto {
            sea_orm_connect_options: options,
            table_name: "bus_events".to_string(),
            table_name_archive: "2343#@@#@$#@#$".to_string(),
        })
        .unwrap_err();

        assert!(matches!(err, BusError::InvalidTableName(_)));
    }

    #[test]
    fn test_database_queue_configuration_valid() {
        let config = DatabaseQueueConfiguration::new(DatabaseQueueConfigurationDto {
            queue_name: "queue".to_string(),
            workers: 2,
            batch_size: 10,
            sleep_interval: Duration::from_secs(5),
            archive_type: ArchiveType::Completed,
        })
        .unwrap();

        assert_eq!(config.queue_name(), "queue");
        assert_eq!(config.workers(), 2);
        assert_eq!(config.batch_size(), 10);
        assert_eq!(config.sleep_interval(), Duration::from_secs(5));
        assert_eq!(config.archive_type(), &ArchiveType::Completed);
        assert!(config.should_archive_success());
        assert!(!config.should_archive_failed());
    }

    #[test]
    fn test_database_queue_configuration_invalid() {
        let err = DatabaseQueueConfiguration::new(DatabaseQueueConfigurationDto {
            queue_name: "queue".to_string(),
            workers: 5,
            batch_size: 0,
            sleep_interval: Duration::from_secs(5),
            archive_type: ArchiveType::None,
        })
        .unwrap_err();
        assert_eq!(
            err,
            BusError::InvalidDatabaseQueueConfiguration(
                "batch_size and workers must be >= 1".to_string()
            )
        );

        let err = DatabaseQueueConfiguration::new(DatabaseQueueConfigurationDto {
            queue_name: "queue".to_string(),
            workers: 0,
            batch_size: 5,
            sleep_interval: Duration::from_secs(5),
            archive_type: ArchiveType::None,
        })
        .unwrap_err();
        assert_eq!(
            err,
            BusError::InvalidDatabaseQueueConfiguration(
                "batch_size and workers must be >= 1".to_string()
            )
        );
    }

    #[test]
    fn test_should_archive_flags() {
        let all = DatabaseQueueConfiguration::new(DatabaseQueueConfigurationDto {
            queue_name: "queue".to_string(),
            workers: 5,
            batch_size: 5,
            sleep_interval: Duration::from_secs(5),
            archive_type: ArchiveType::All,
        })
        .unwrap();
        assert!(all.should_archive_failed());
        assert!(all.should_archive_success());

        let failed = DatabaseQueueConfiguration::new(DatabaseQueueConfigurationDto {
            queue_name: "queue".to_string(),
            workers: 5,
            batch_size: 5,
            sleep_interval: Duration::from_secs(5),
            archive_type: ArchiveType::Failed,
        })
        .unwrap();
        assert!(failed.should_archive_failed());
        assert!(!failed.should_archive_success());

        let completed = DatabaseQueueConfiguration::new(DatabaseQueueConfigurationDto {
            queue_name: "queue".to_string(),
            workers: 5,
            batch_size: 5,
            sleep_interval: Duration::from_secs(5),
            archive_type: ArchiveType::Completed,
        })
        .unwrap();
        assert!(!completed.should_archive_failed());
        assert!(completed.should_archive_success());

        let none = DatabaseQueueConfiguration::new(DatabaseQueueConfigurationDto {
            queue_name: "queue".to_string(),
            workers: 5,
            batch_size: 5,
            sleep_interval: Duration::from_secs(5),
            archive_type: ArchiveType::None,
        })
        .unwrap();
        assert!(!none.should_archive_failed());
        assert!(!none.should_archive_success());
    }

    #[test]
    fn test_database_queue_processor_configuration() {
        let options = ConnectOptions::new("sqlite::memory:");
        let conn = DatabaseConnection::new(DatabaseConnectionDto {
            sea_orm_connect_options: options,
            table_name: "bus_events".to_string(),
            table_name_archive: "bus_events_archive".to_string(),
        })
        .unwrap();

        let queue = DatabaseQueueConfiguration::new(DatabaseQueueConfigurationDto {
            queue_name: "queue".to_string(),
            workers: 5,
            batch_size: 5,
            sleep_interval: Duration::from_secs(5),
            archive_type: ArchiveType::None,
        })
        .unwrap();

        let config = DatabaseQueueProcessorConfiguration::new(conn, vec![queue.clone()]);
        assert_eq!(config.queues().len(), 1);
        assert_eq!(config.queues()[0].queue_name(), "queue");
        assert_eq!(config.connection().table_name(), "bus_events");
    }
}
