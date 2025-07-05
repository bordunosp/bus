use crate::core::contracts::{
    AnyError, EventQueueSettings, IErasedEventHandlerDatabase, IEvent, IEventHandlerDatabase,
};
use crate::core::error_bus::BusError;
use crate::core::features::sea_orm::db_enums::EventStatusEnum;
use crate::core::features::sea_orm::dto::BusEvent;
use crate::core::features::sea_orm::initialization::{get_db_conn, get_queue_config};
use crate::core::features::sea_orm::sql;
use async_trait::async_trait;
use chrono::Utc;
use dashmap::{DashMap, Entry};
use once_cell::sync::OnceCell;
use sea_orm::DatabaseTransaction;
use sea_orm::TransactionTrait;
use std::any::Any;
use std::any::type_name;
use std::error::Error;
use std::pin::Pin;
use std::sync::Arc;
use uuid::Uuid;

pub(crate) struct HandlerRegistration {
    pub(crate) factory: Arc<AsyncEventDbFactory>,
    pub(crate) settings: EventQueueSettings,
    pub(crate) calculate_scheduled_at: fn(i32) -> chrono::Duration,
}

type AsyncEventDbFactory = dyn Fn() -> Pin<
        Box<
            dyn Future<Output = Result<Box<dyn IErasedEventHandlerDatabase>, Box<dyn Error>>>
                + Send,
        >,
    > + Send
    + Sync;

type EventFromBinFactory =
    dyn Fn(&Vec<u8>) -> Result<Box<dyn Any + Send + Sync>, Box<dyn Error>> + Send + Sync;

static HANDLER_REGISTRY: OnceCell<DashMap<&'static str, HandlerRegistration>> = OnceCell::new();
static EVENT_TO_HANDLERS: OnceCell<DashMap<&'static str, Vec<&'static str>>> = OnceCell::new();
static EVENT_BIN: OnceCell<DashMap<&'static str, Arc<EventFromBinFactory>>> = OnceCell::new();

pub(crate) fn handler_registry() -> &'static DashMap<&'static str, HandlerRegistration> {
    HANDLER_REGISTRY.get_or_init(DashMap::new)
}

pub(crate) fn event_to_handlers() -> &'static DashMap<&'static str, Vec<&'static str>> {
    EVENT_TO_HANDLERS.get_or_init(DashMap::new)
}

pub(crate) fn event_from_bin() -> &'static DashMap<&'static str, Arc<EventFromBinFactory>> {
    EVENT_BIN.get_or_init(DashMap::new)
}

struct EventHandlerDatabaseWrapper<H, E, TError>
where
    H: IEventHandlerDatabase<E, TError>,
    E: IEvent<TError>,
    TError: AnyError,
{
    inner: H,
    _phantom: std::marker::PhantomData<(E, TError)>,
}

impl<H, E, TError> EventHandlerDatabaseWrapper<H, E, TError>
where
    H: IEventHandlerDatabase<E, TError>,
    E: IEvent<TError>,
    TError: AnyError,
{
    pub fn new(handler: H) -> Self {
        Self {
            inner: handler,
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<H, E, TError> IErasedEventHandlerDatabase for EventHandlerDatabaseWrapper<H, E, TError>
where
    H: IEventHandlerDatabase<E, TError>,
    E: IEvent<TError>,
    TError: AnyError,
{
    async fn handle(
        &self,
        event: Box<dyn Any + Send + Sync>,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let evt = event.downcast::<E>().map_err(|_| {
            BusError::EventDatabaseIncorrectRequestType(
                type_name::<E>().to_string(),
                type_name::<H>().to_string(),
            )
        })?;

        self.inner.handle_async(*evt).await?;
        Ok(())
    }
}

pub(crate) fn register_event_database_handler<H, E, TError, F, Fut>(
    factory: F,
) -> Result<(), BusError>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<H, TError>> + Send + 'static,
    H: IEventHandlerDatabase<E, TError>,
    E: IEvent<TError>,
    TError: AnyError,
{
    let event_type_name = type_name::<E>();
    let handler_type_name = type_name::<H>();

    match handler_registry().entry(handler_type_name) {
        Entry::Occupied(_) => {
            return Err(BusError::EventHandlerDatabaseRegistered(
                event_type_name.to_string(),
            ));
        }
        Entry::Vacant(entry) => {
            entry.insert(HandlerRegistration {
                factory: Arc::new(move || {
                    let fut = factory();
                    Box::pin(async move {
                        let handler = fut.await?;
                        Ok::<_, Box<dyn Error>>(Box::new(EventHandlerDatabaseWrapper::new(handler))
                            as Box<dyn IErasedEventHandlerDatabase>)
                    })
                }),
                settings: H::settings(),
                calculate_scheduled_at: H::calculate_scheduled_at,
            });
        }
    }

    event_to_handlers()
        .entry(event_type_name)
        .and_modify(|handlers| {
            if !handlers.contains(&handler_type_name) {
                handlers.push(handler_type_name);
            }
        })
        .or_insert_with(|| vec![handler_type_name]);

    event_from_bin().entry(event_type_name).or_insert_with(|| {
        Arc::new(|bytes: &Vec<u8>| {
            let event: E = postcard::from_bytes(bytes).map_err(|e| {
                Box::new(BusError::DeserializationError(
                    type_name::<E>().to_string(),
                    e.to_string(),
                )) as Box<dyn Error>
            })?;
            Ok(Box::new(event) as Box<dyn Any + Send + Sync>)
        })
    });

    Ok(())
}

pub(crate) async fn insert_pending_bus_events<E, TError>(event: E) -> Result<(), BusError>
where
    E: IEvent<TError>,
    TError: AnyError,
{
    insert_pending_bus_events_txn::<E, TError>(None, event).await?;
    Ok(())
}

pub(crate) async fn insert_pending_bus_events_txn<E, TError>(
    db: Option<DatabaseTransaction>,
    event: E,
) -> Result<(), BusError>
where
    E: IEvent<TError>,
    TError: AnyError,
{
    let event_type_name = type_name::<E>();

    let Some(handlers) = event_to_handlers().get(event_type_name) else {
        return Ok(());
    };

    #[cfg(feature = "json-payload")]
    let payload_json = serde_json::to_value(&event)
        .map_err(|e| BusError::SerializationError(event_type_name.to_string(), e.to_string()))?;

    let payload_bin = postcard::to_allocvec(&event)
        .map_err(|e| BusError::SerializationError(event_type_name.to_string(), e.to_string()))?;

    let txn = match db {
        Some(txn) => txn,
        None => get_db_conn()?.begin().await.map_err(BusError::DbErr)?,
    };

    let now = Utc::now().naive_utc();

    for &handler_type_name in handlers.iter() {
        let Some(event_queue_settings) = handler_registry().get(handler_type_name) else {
            return Err(BusError::EventQueueSettingsNotFoundConfig(
                handler_type_name.to_string(),
            ));
        };

        let Some(bus_queue_config) = get_queue_config()?
            .queues()
            .iter()
            .find(|c| c.queue_name() == event_queue_settings.settings.queue_name)
        else {
            return Err(BusError::EventQueueSettingsNotFoundConfig(
                handler_type_name.to_string(),
            ));
        };

        sql::insert_to_events(
            &txn,
            BusEvent {
                id: Uuid::now_v7(),
                queue_name: event_queue_settings.settings.queue_name.to_string(),
                type_name_event: event_type_name.to_string(),
                type_name_handler: handler_type_name.to_string(),
                status: EventStatusEnum::Pending,

                #[cfg(feature = "json-payload")]
                payload_json: payload_json.clone(),

                payload_bin: payload_bin.clone(),
                retries_current: 0,
                retries_max: event_queue_settings.settings.max_retries,
                latest_error: None,
                archive_mode: bus_queue_config.archive_type().clone(),
                should_start_at: now,
                expires_at: None,
                expires_interval: event_queue_settings.settings.execution_timeout,
                created_at: now,
                updated_at: now,
            },
        )
        .await?;
    }

    txn.commit().await.map_err(BusError::DbErr)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::contracts::*;
    use crate::core::error_bus::BusError;
    use crate::core::features::sea_orm::core::{
        ArchiveType, DatabaseConnection, DatabaseQueueConfiguration,
        DatabaseQueueProcessorConfiguration,
    };
    use crate::core::features::sea_orm::dto::{
        DatabaseConnectionDto, DatabaseQueueConfigurationDto,
    };
    use crate::core::features::sea_orm::event_handlers_database::{
        EVENT_BIN, EVENT_TO_HANDLERS, HANDLER_REGISTRY, insert_pending_bus_events_txn,
        register_event_database_handler,
    };
    use crate::core::features::sea_orm::initialization::init_sea_orm_for_tests;
    use sea_orm::{
        ConnectOptions, DatabaseBackend, DatabaseConnection as SeaOrmConnection, MockDatabase,
        MockExecResult,
    };
    use std::time::Duration;

    #[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
    struct TestEventNotRegistered {
        pub message2: bool,
    }

    impl<T> IEvent<T> for TestEventNotRegistered where T: AnyError {}

    #[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
    struct TestEvent {
        pub message: String,
    }

    impl<T> IEvent<T> for TestEvent where T: AnyError {}

    #[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
    struct TestEventMissingQueue {
        pub message: String,
    }

    impl<T> IEvent<T> for TestEventMissingQueue where T: AnyError {}

    #[derive(Debug)]
    struct TestError;

    impl std::fmt::Display for TestError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "TestError")
        }
    }

    impl Error for TestError {}
    impl From<BusError> for TestError {
        fn from(_: BusError) -> Self {
            TestError
        }
    }

    struct DummyHandler;

    #[async_trait::async_trait]
    impl IEventHandlerDatabase<TestEvent, TestError> for DummyHandler {
        fn settings() -> EventQueueSettings {
            EventQueueSettings {
                queue_name: "test-queue".to_string(),
                max_retries: 3,
                execution_timeout: chrono::Duration::seconds(60),
            }
        }

        fn calculate_scheduled_at(_retry: i32) -> chrono::Duration {
            chrono::Duration::minutes(12)
        }

        async fn handle_async(&self, _event: TestEvent) -> Result<(), TestError> {
            Ok(())
        }
    }

    struct DummyHandlerMissingQueue;

    #[async_trait::async_trait]
    impl IEventHandlerDatabase<TestEventMissingQueue, TestError> for DummyHandlerMissingQueue {
        fn settings() -> EventQueueSettings {
            EventQueueSettings {
                queue_name: "missing".to_string(),
                max_retries: 3,
                execution_timeout: chrono::Duration::seconds(60),
            }
        }

        fn calculate_scheduled_at(_retry: i32) -> chrono::Duration {
            chrono::Duration::seconds(123)
        }

        async fn handle_async(&self, _event: TestEventMissingQueue) -> Result<(), TestError> {
            Ok(())
        }
    }

    fn setup_processor_config() -> (SeaOrmConnection, DatabaseQueueProcessorConfiguration) {
        let mock_conn: SeaOrmConnection = MockDatabase::new(DatabaseBackend::Postgres)
            .append_exec_results([MockExecResult {
                last_insert_id: 1,
                rows_affected: 1,
            }])
            .into_connection();

        let connect_options = ConnectOptions::new("sqlite::memory:");

        let db_config = DatabaseConnection::new(DatabaseConnectionDto {
            sea_orm_connect_options: connect_options,
            table_name: "bus_event".to_string(),
            table_name_archive: "bus_event_archive".to_string(),
        })
        .unwrap();

        let queue_config = DatabaseQueueConfiguration::new(DatabaseQueueConfigurationDto {
            queue_name: "test-queue".to_string(),
            workers: 1,
            batch_size: 10,
            sleep_interval: Duration::from_secs(1),
            archive_type: ArchiveType::None,
        })
        .unwrap();

        let processor_config =
            DatabaseQueueProcessorConfiguration::new(db_config, vec![queue_config]);

        (mock_conn, processor_config)
    }

    #[tokio::test]
    async fn test_insert_pending_bus_events_txn_with_handler() {
        HANDLER_REGISTRY.get_or_init(DashMap::new).clear();
        EVENT_TO_HANDLERS.get_or_init(DashMap::new).clear();
        EVENT_BIN.get_or_init(DashMap::new).clear();

        let (mock_conn, processor_config) = setup_processor_config();
        init_sea_orm_for_tests(mock_conn, processor_config).unwrap();

        register_event_database_handler::<DummyHandler, TestEvent, TestError, _, _>(|| async {
            Ok(DummyHandler)
        })
        .unwrap();

        let event = TestEvent {
            message: "Hello".to_string(),
        };

        let result = insert_pending_bus_events_txn::<TestEvent, TestError>(None, event).await;

        assert!(
            result.is_ok(),
            "insert_pending_bus_events_txn failed: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn test_insert_pending_bus_events_txn_no_handlers() {
        HANDLER_REGISTRY.get_or_init(DashMap::new).clear();
        EVENT_TO_HANDLERS.get_or_init(DashMap::new).clear();
        EVENT_BIN.get_or_init(DashMap::new).clear();

        let (mock_conn, processor_config) = setup_processor_config();
        init_sea_orm_for_tests(mock_conn, processor_config).unwrap();

        let event = TestEvent {
            message: "No handler".to_string(),
        };

        let result = insert_pending_bus_events_txn::<TestEvent, TestError>(None, event).await;

        assert!(result.is_ok(), "expected Ok, got {:?}", result);
    }

    #[tokio::test]
    async fn test_register_duplicate_handler_fails() {
        HANDLER_REGISTRY.get_or_init(DashMap::new).clear();

        let result1 =
            register_event_database_handler::<DummyHandler, TestEvent, TestError, _, _>(|| async {
                Ok(DummyHandler)
            });
        let result2 =
            register_event_database_handler::<DummyHandler, TestEvent, TestError, _, _>(|| async {
                Ok(DummyHandler)
            });

        assert!(result1.is_ok());
        assert!(result2.is_err());
    }

    #[tokio::test]
    async fn test_insert_event_missing_queue_config_fails() {
        HANDLER_REGISTRY.get_or_init(DashMap::new).clear();
        EVENT_TO_HANDLERS.get_or_init(DashMap::new).clear();
        EVENT_BIN.get_or_init(DashMap::new).clear();

        let (mock_conn, processor_config) = setup_processor_config();

        init_sea_orm_for_tests(mock_conn, processor_config).unwrap();

        register_event_database_handler::<
            DummyHandlerMissingQueue,
            TestEventMissingQueue,
            TestError,
            _,
            _,
        >(|| async { Ok(DummyHandlerMissingQueue) })
        .unwrap();

        let event = TestEventMissingQueue {
            message: "Missing queue config".to_string(),
        };

        let result =
            insert_pending_bus_events_txn::<TestEventMissingQueue, TestError>(None, event).await;

        assert!(
            matches!(result, Err(BusError::EventQueueSettingsNotFoundConfig(_))),
            "Expected missing queue config error, got: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn test_event_json_deserialization_works() {
        use crate::core::features::sea_orm::event_handlers_database::event_from_bin;

        HANDLER_REGISTRY.get_or_init(DashMap::new).clear();
        EVENT_TO_HANDLERS.get_or_init(DashMap::new).clear();
        EVENT_BIN.get_or_init(DashMap::new).clear();

        register_event_database_handler::<DummyHandler, TestEvent, TestError, _, _>(|| async {
            Ok(DummyHandler)
        })
        .unwrap();

        let type_name = type_name::<TestEvent>();
        let factory = event_from_bin()
            .get(type_name)
            .expect("Deserializer should be registered");

        let event = TestEvent {
            message: "From BIN".to_string(),
        };
        let bin_data = postcard::to_allocvec(&event).unwrap();

        let result = factory(&bin_data);
        assert!(result.is_ok(), "Deserialization failed: {:?}", result);

        let boxed = result.unwrap();
        let downcasted_box = boxed.downcast::<TestEvent>();
        assert!(downcasted_box.is_ok(), "Failed to downcast to TestEvent");
        let event = *downcasted_box.unwrap();
        assert_eq!(event.message, "From BIN");
    }

    #[tokio::test]
    async fn test_event_bin_factory_not_registered() {
        use crate::core::features::sea_orm::event_handlers_database::event_from_bin;

        HANDLER_REGISTRY.get_or_init(DashMap::new).clear();
        EVENT_TO_HANDLERS.get_or_init(DashMap::new).clear();
        EVENT_BIN.get_or_init(DashMap::new).clear();

        register_event_database_handler::<DummyHandler, TestEvent, TestError, _, _>(|| async {
            Ok(DummyHandler)
        })
        .unwrap();

        let type_name = type_name::<TestEventNotRegistered>();
        let factory = event_from_bin().get(type_name);

        assert!(
            factory.is_none(),
            "Expected no factory for unregistered type `{}`",
            type_name
        );
    }

    #[tokio::test]
    async fn test_event_bin_factory_wrong_type_fails() {
        use crate::core::features::sea_orm::event_handlers_database::event_from_bin;

        HANDLER_REGISTRY.get_or_init(DashMap::new).clear();
        EVENT_TO_HANDLERS.get_or_init(DashMap::new).clear();
        EVENT_BIN.get_or_init(DashMap::new).clear();

        register_event_database_handler::<DummyHandler, TestEvent, TestError, _, _>(|| async {
            Ok(DummyHandler)
        })
        .unwrap();

        let type_name = type_name::<TestEvent>();
        let factory = event_from_bin()
            .get(type_name)
            .expect("Deserializer should be registered");

        let wrong_event = TestEventNotRegistered { message2: true };
        let bin_data = postcard::to_allocvec(&wrong_event).unwrap();

        let result = factory(&bin_data);

        assert!(
            result.is_err(),
            "Expected deserialization to fail due to type mismatch, but got: {:?}",
            result
        );
    }
}
