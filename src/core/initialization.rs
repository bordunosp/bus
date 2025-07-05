#[cfg(any(feature = "sea-orm-postgres", feature = "sea-orm-mysql"))]
use {
    crate::core::error_bus::BusError, crate::core::features::sea_orm as features_sea_orm,
    crate::core::features::sea_orm::core::DatabaseQueueProcessorConfiguration,
};

#[cfg(any(feature = "sea-orm-postgres", feature = "sea-orm-mysql"))]
pub async fn sea_orm(
    queue_configuration: DatabaseQueueProcessorConfiguration,
) -> Result<(), BusError> {
    features_sea_orm::initialization::init_sea_orm(queue_configuration).await
}

#[cfg(test)]
#[cfg(any(feature = "sea-orm-postgres", feature = "sea-orm-mysql"))]
mod tests {
    use super::*;
    use crate::core::features::sea_orm::core::*;
    use crate::core::features::sea_orm::dto::{
        DatabaseConnectionDto, DatabaseQueueConfigurationDto,
    };
    use sea_orm::ConnectOptions;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    #[cfg(any(feature = "sea-orm-postgres", feature = "sea-orm-mysql"))]
    #[async_trait::async_trait]
    pub trait SeaOrmInitializer: Send + Sync {
        async fn init(&self, config: DatabaseQueueProcessorConfiguration) -> Result<(), BusError>;
    }

    #[cfg(any(feature = "sea-orm-postgres", feature = "sea-orm-mysql"))]
    pub struct DefaultSeaOrmInitializer;

    #[cfg(any(feature = "sea-orm-postgres", feature = "sea-orm-mysql"))]
    #[async_trait::async_trait]
    impl SeaOrmInitializer for DefaultSeaOrmInitializer {
        async fn init(&self, config: DatabaseQueueProcessorConfiguration) -> Result<(), BusError> {
            features_sea_orm::initialization::init_sea_orm(config).await
        }
    }

    #[cfg(any(feature = "sea-orm-postgres", feature = "sea-orm-mysql"))]
    pub async fn sea_orm_with<I: SeaOrmInitializer>(
        config: DatabaseQueueProcessorConfiguration,
        initializer: &I,
    ) -> Result<(), BusError> {
        initializer.init(config).await
    }

    struct MockInitializer {
        called: Arc<AtomicBool>,
        should_fail: bool,
    }

    #[async_trait::async_trait]
    impl SeaOrmInitializer for MockInitializer {
        async fn init(&self, _config: DatabaseQueueProcessorConfiguration) -> Result<(), BusError> {
            self.called.store(true, Ordering::SeqCst);
            if self.should_fail {
                Err(BusError::DbErr(sea_orm::DbErr::Custom("fail".into())))
            } else {
                Ok(())
            }
        }
    }

    fn dummy_config() -> DatabaseQueueProcessorConfiguration {
        let mut options = ConnectOptions::new("postgres://localhost");
        options.sqlx_logging(false);

        let conn = DatabaseConnection::new(DatabaseConnectionDto {
            sea_orm_connect_options: options,
            table_name: "events".to_string(),
            table_name_archive: "events_archive".to_string(),
        })
        .unwrap();

        let queue = DatabaseQueueConfiguration::new(DatabaseQueueConfigurationDto {
            queue_name: "test-queue".to_string(),
            workers: 1,
            batch_size: 10,
            sleep_interval: Duration::from_secs(1),
            archive_type: ArchiveType::None,
        })
        .unwrap();

        DatabaseQueueProcessorConfiguration::new(conn, vec![queue])
    }

    #[tokio::test]
    async fn test_sea_orm_wrapper_success_mocked() {
        let called = Arc::new(AtomicBool::new(false));
        let mock = MockInitializer {
            called: called.clone(),
            should_fail: false,
        };

        let result = sea_orm_with(dummy_config(), &mock).await;
        assert!(result.is_ok());
        assert!(called.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_sea_orm_wrapper_failure_mocked() {
        let called = Arc::new(AtomicBool::new(false));
        let mock = MockInitializer {
            called: called.clone(),
            should_fail: true,
        };

        let result = sea_orm_with(dummy_config(), &mock).await;
        assert!(result.is_err());
        assert!(called.load(Ordering::SeqCst));
    }
}
