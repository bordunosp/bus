#[cfg(test)]
mod sea_orm_integration_tests {
    use crate as rust_bus;
    use crate::bus;
    use crate::contracts::event_handler_database::IEventHandlerDatabase;
    use crate::contracts::meta::BusMetadata;
    use crate::tests::base::setup_bus;
    use crate::workers::configuration::BusQueueConfiguration;
    use chrono::Utc;
    use rust_bus::{BusEvent, BusEventHandlerDatabase};
    use sea_orm::ColumnTrait;
    use sea_orm::PaginatorTrait;
    use sea_orm::prelude::Expr;
    use sea_orm::{ActiveModelTrait, QueryFilter};
    use sea_orm::{ConnectionTrait, DatabaseConnection, EntityTrait, Statement, TransactionTrait};
    use std::hash::Hasher;
    use twox_hash::XxHash3_64;
    use uuid::Uuid;

    #[BusEvent]
    pub struct OrderCreated {
        pub id: i32,
    }

    mod internal {
        use super::*;
        #[BusEvent]
        pub struct OrderCancelled {
            pub order_id: i32,
        }
    }

    use crate::contracts::database_unique::Unique;
    use crate::contracts::enums::{
        AvailableField, ExecutingField, Field, Period, Replace, RetryableField, ScheduleIn,
        ScheduledField, State, TerminalField, Timestamp,
    };
    use crate::models::bus_jobs;
    use crate::models::sea_orm_active_enums::BusJobState;
    use internal::OrderCancelled as CancelledEvent;

    #[tokio::test]
    async fn test_event_identity_with_alias() {
        setup_bus().await;
        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let txn = pool.begin().await.unwrap();

        txn.execute_raw(Statement::from_string(
            txn.get_database_backend(),
            "DELETE FROM bus_jobs",
        ))
        .await
        .unwrap();

        let event = CancelledEvent { order_id: 777 };

        #[cfg(feature = "context")]
        let ctx = ExampleBusContext::new(&txn, BusMetadata::default());
        #[cfg(feature = "context")]
        bus::event(&ctx, event).await.unwrap();

        #[cfg(not(feature = "context"))]
        bus::event(&txn, event, &BusMetadata::default())
            .await
            .unwrap();

        let job = bus_jobs::Entity::find()
            .filter(bus_jobs::Column::TypeNameHandler.eq(OrderHandlerUseAs::HANDLER_IDENTITY))
            .one(&txn)
            .await
            .expect("Job not found")
            .unwrap();

        assert_eq!(
            CancelledEvent::EVENT_IDENTITY,
            "rust_bus::tests::dispatch_db_sea_orm::sea_orm_integration_tests::internal::OrderCancelled"
        );

        assert_eq!(
            job.type_name_event,
            "rust_bus::tests::dispatch_db_sea_orm::sea_orm_integration_tests::internal::OrderCancelled",
            "The database must store the original struct name, not the alias"
        );

        assert_ne!(
            job.type_name_event, "CancelledEvent",
            "Alias name should never be used as identity"
        );

        txn.rollback().await.unwrap();
    }

    #[derive(Default)]
    pub struct OrderHandler;

    #[BusEventHandlerDatabase]
    impl IEventHandlerDatabase<OrderCreated> for OrderHandler {
        const QUEUE: &'static str = "default";
        const PRIORITY: u32 = 10;
        const MAX_ATTEMPTS: Option<u32> = Some(22);
        const EXECUTION_TIMEOUT: Option<chrono::Duration> = Some(chrono::Duration::minutes(14));
        const SCHEDULE_IN: ScheduleIn = ScheduleIn::Duration(std::time::Duration::from_hours(33));
        const TAGS: &'static [&'static str] = &["user", "company"];
        const UNIQUE: Option<Unique> = Some(Unique {
            period: Period::Duration {
                field: Timestamp::InsertedAt,
                duration: std::time::Duration::from_hours(7),
            },
            fields: &[Field::Event],
            keys: &["user_id"],
            states: &[State::Available, State::Scheduled],
            replace: &[Replace::Available(&[AvailableField::Meta])],
        });

        async fn handle(
            &self,
            _db: &DatabaseConnection,
            _event: &OrderCreated,
            _metadata: &BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    #[derive(Default)]
    pub struct OrderHandlerDefault;

    #[BusEventHandlerDatabase]
    impl IEventHandlerDatabase<OrderCreated> for OrderHandlerDefault {
        const QUEUE: &'static str = "default";

        async fn handle(
            &self,
            _db: &DatabaseConnection,
            _event: &OrderCreated,
            _metadata: &BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    #[derive(Default)]
    pub struct OrderHandlerUseAs;

    #[BusEventHandlerDatabase]
    impl IEventHandlerDatabase<internal::OrderCancelled> for OrderHandlerUseAs {
        const QUEUE: &'static str = "default";

        async fn handle(
            &self,
            _db: &DatabaseConnection,
            _event: &internal::OrderCancelled,
            _metadata: &BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_save_to_db() {
        setup_bus().await;
        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let txn = pool.begin().await.unwrap();

        txn.execute_raw(Statement::from_string(
            txn.get_database_backend(),
            "DELETE FROM bus_jobs",
        ))
        .await
        .unwrap();

        let event = OrderCreated { id: 100 };
        let mut meta = BusMetadata::default();
        meta.add("user_id", 42).unwrap();
        meta.add("info", "first_attempt".to_string()).unwrap();
        meta.add("request_id", Uuid::now_v7()).unwrap();
        meta.add("correlation_id", Uuid::now_v7()).unwrap();
        meta.add("causation_id", Uuid::now_v7()).unwrap();

        #[cfg(feature = "context")]
        bus::event(&ExampleBusContext::new(&txn, meta.clone()), event.clone())
            .await
            .unwrap();

        #[cfg(not(feature = "context"))]
        bus::event(&txn, event.clone(), &meta.clone())
            .await
            .unwrap();

        let job = bus_jobs::Entity::find()
            .filter(bus_jobs::Column::TypeNameHandler.eq(OrderHandler::HANDLER_IDENTITY))
            .filter(bus_jobs::Column::TypeNameEvent.eq(OrderCreated::EVENT_IDENTITY))
            .one(&txn)
            .await
            .expect("Job not found")
            .unwrap();

        assert_eq!(
            job.id.get_version(),
            Some(uuid::Version::SortRand),
            "Job ID must be a UUID v7 for proper sorting"
        );

        assert_eq!(
            job.type_name_handler,
            "rust_bus::tests::dispatch_db_sea_orm::sea_orm_integration_tests::OrderHandler"
        );
        assert_eq!(
            job.type_name_event,
            "rust_bus::tests::dispatch_db_sea_orm::sea_orm_integration_tests::OrderCreated"
        );
        assert_eq!(job.queue, "default");

        assert_eq!(job.state, BusJobState::Available);
        assert_eq!(job.priority, 10);
        assert_eq!(job.attempt, 0);
        assert_eq!(job.max_attempts, 22);
        assert_eq!(job.execution_timeout_sec, 14 * 60);

        let inserted_at: chrono::DateTime<Utc> = job.inserted_at;
        let scheduled_at: chrono::DateTime<Utc> = job.scheduled_at;
        assert!(inserted_at <= Utc::now());

        let expected_scheduled_at = inserted_at + chrono::Duration::hours(33);
        let diff = scheduled_at - expected_scheduled_at;
        assert!(
            diff.num_seconds().abs() < 5,
            "scheduled_at ({}) should be exactly 33h after inserted_at ({})",
            scheduled_at,
            inserted_at
        );

        for field_name in vec![
            job.attempted_at,
            job.completed_at,
            job.discarded_at,
            job.cancelled_at,
        ] {
            assert!(field_name.is_none());
        }

        let meta_from_json: BusMetadata =
            serde_json::from_str(job.meta.to_string().as_str()).unwrap();

        assert_eq!(job.payload["id"], 100);
        assert_eq!(meta_from_json.get("user_id"), Some(42));
        assert_eq!(
            meta_from_json.get("info"),
            Some("first_attempt".to_string())
        );
        assert_eq!(
            meta_from_json.get::<Uuid>("request_id"),
            meta.get::<Uuid>("request_id")
        );

        let tags_array = job.tags.as_array().expect("Tags should be an array");
        assert!(tags_array.iter().any(|t| t.as_str() == Some("user")));
        assert!(tags_array.iter().any(|t| t.as_str() == Some("company")));

        assert!(job.errors.as_array().unwrap().is_empty());
        assert!(job.attempted_by.as_array().unwrap().is_empty());

        {
            let job = bus_jobs::Entity::find()
                .filter(bus_jobs::Column::TypeNameHandler.eq(OrderHandlerDefault::HANDLER_IDENTITY))
                .filter(bus_jobs::Column::TypeNameEvent.eq(OrderCreated::EVENT_IDENTITY))
                .one(&txn)
                .await
                .expect("Job not found")
                .unwrap();

            assert_eq!(job.priority, 0);
            assert_eq!(job.max_attempts, 3);
            assert_eq!(job.execution_timeout_sec, 10 * 60);
            assert!(job.inserted_at <= Utc::now());

            let diff = job.scheduled_at - Utc::now();
            assert!(
                diff.num_seconds().abs() < 5,
                "For default handler, scheduled_at must be equal to inserted_at"
            );
        }

        txn.rollback().await.unwrap();

        println!("✅ All fields verified successfully");
    }

    #[BusEvent]
    pub struct StateTestEvent {
        pub code: String,
    }

    #[derive(Default)]
    pub struct HandlerAvailable;
    #[BusEventHandlerDatabase]
    impl IEventHandlerDatabase<StateTestEvent> for HandlerAvailable {
        const QUEUE: &'static str = "default";
        const PRIORITY: u32 = 21;
        const MAX_ATTEMPTS: Option<u32> = Some(100);
        const EXECUTION_TIMEOUT: Option<chrono::Duration> = Some(chrono::Duration::seconds(333));
        const SCHEDULE_IN: ScheduleIn = ScheduleIn::Immediately;
        const TAGS: &'static [&'static str] = &["11", "22", "33"];

        const UNIQUE: Option<Unique> = Some(Unique {
            period: Period::Infinite,
            fields: &[Field::Event],
            keys: &["key_1", "key_2"],
            states: &[State::Available],
            replace: &[Replace::Available(&[
                AvailableField::Queue,
                AvailableField::Priority,
                AvailableField::MaxAttempts,
                AvailableField::ExecutionTimeout,
                AvailableField::Tags,
                AvailableField::Meta,
            ])],
        });

        async fn handle(
            &self,
            _db: &DatabaseConnection,
            _event: &StateTestEvent,
            _metadata: &BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_unique_available_replace() {
        setup_bus().await;
        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let txn = pool.begin().await.unwrap();

        txn.execute_raw(Statement::from_string(
            txn.get_database_backend(),
            "DELETE FROM bus_jobs",
        ))
        .await
        .unwrap();

        let mut hasher = XxHash3_64::with_seed(0);
        hasher.write(HandlerAvailable::HANDLER_IDENTITY.as_bytes());
        hasher.write(StateTestEvent::EVENT_IDENTITY.as_bytes());
        let hash_type_name = hasher.finish() as i64;

        let insert_now = Utc::now() - chrono::Duration::hours(10);

        let active_model = bus_jobs::ActiveModel {
            id: sea_orm::Set(Uuid::now_v7()),
            hash_type_name: sea_orm::Set(hash_type_name),
            inserted_at: sea_orm::Set(insert_now),
            scheduled_at: sea_orm::Set(insert_now),
            attempted_at: sea_orm::Set(None),
            completed_at: sea_orm::Set(None),
            cancelled_at: sea_orm::Set(None),
            discarded_at: sea_orm::Set(None),
            state: sea_orm::Set(BusJobState::Available),
            priority: sea_orm::Set(10),
            attempt: sea_orm::Set(10),
            max_attempts: sea_orm::Set(10),
            execution_timeout_sec: sea_orm::Set(100),
            queue: sea_orm::Set("NOT_DEFAULT".to_string()),
            type_name_event: sea_orm::Set(StateTestEvent::EVENT_IDENTITY.to_string()),
            type_name_handler: sea_orm::Set(HandlerAvailable::HANDLER_IDENTITY.to_string()),
            payload: sea_orm::Set(
                serde_json::to_value(StateTestEvent {
                    code: "UNIQUE_101".to_string(),
                })
                .expect("Should be serializable"),
            ),
            meta: sea_orm::Set(
                serde_json::to_value(BusMetadata::default()).expect("Should be serializable"),
            ),
            tags: sea_orm::Set(serde_json::json!(["1", "2", "3"])),
            errors: sea_orm::Set(serde_json::json!([])),
            attempted_by: sea_orm::Set(serde_json::json!([])),
        };

        active_model.insert(&txn).await.unwrap();

        let event = StateTestEvent {
            code: "UNIQUE_101".to_string(),
        };

        let mut meta = BusMetadata::default();
        meta.add("attempt", 1).unwrap();
        meta.add("request_id", Uuid::now_v7()).unwrap();
        meta.add("correlation_id", Uuid::now_v7()).unwrap();
        meta.add("causation_id", Uuid::now_v7()).unwrap();
        meta.add("flatten_val", Uuid::now_v7()).unwrap();

        #[cfg(feature = "context")]
        bus::event(&ExampleBusContext::new(&txn, meta.clone()), event.clone())
            .await
            .unwrap();

        #[cfg(not(feature = "context"))]
        bus::event(&txn, event.clone(), &meta).await.unwrap();

        let job = bus_jobs::Entity::find()
            .filter(bus_jobs::Column::TypeNameHandler.eq(HandlerAvailable::HANDLER_IDENTITY))
            .filter(bus_jobs::Column::TypeNameEvent.eq(StateTestEvent::EVENT_IDENTITY))
            .one(&txn)
            .await
            .expect("Job not found")
            .unwrap();

        assert_eq!(job.queue, "default".to_string());
        assert_eq!(job.priority, 21);
        assert_eq!(job.max_attempts, 100);
        assert_eq!(job.execution_timeout_sec, 333);
        assert_eq!(job.tags, serde_json::json!(["11", "22", "33"]));
        assert_eq!(job.meta, serde_json::to_value(meta).unwrap());

        txn.rollback().await.unwrap();
    }

    #[derive(Default)]
    pub struct HandlerScheduled;
    #[BusEventHandlerDatabase]
    impl IEventHandlerDatabase<StateTestEvent> for HandlerScheduled {
        const QUEUE: &'static str = "default";
        const PRIORITY: u32 = 21;
        const MAX_ATTEMPTS: Option<u32> = Some(100);
        const EXECUTION_TIMEOUT: Option<chrono::Duration> = Some(chrono::Duration::seconds(333));
        const SCHEDULE_IN: ScheduleIn = ScheduleIn::Immediately;
        const TAGS: &'static [&'static str] = &["11", "22", "33"];

        const UNIQUE: Option<Unique> = Some(Unique {
            period: Period::Infinite,
            fields: &[Field::Queue],
            keys: &["key_1", "key_2"],
            states: &[State::Scheduled],
            replace: &[Replace::Scheduled(&[
                ScheduledField::Event,
                ScheduledField::Priority,
                ScheduledField::MaxAttempts,
                ScheduledField::ExecutionTimeout,
                ScheduledField::Tags,
                ScheduledField::Meta,
            ])],
        });

        async fn handle(
            &self,
            _db: &DatabaseConnection,
            _event: &StateTestEvent,
            _metadata: &BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_unique_scheduled_replace() {
        setup_bus().await;
        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let txn = pool.begin().await.unwrap();

        txn.execute_raw(Statement::from_string(
            txn.get_database_backend(),
            "DELETE FROM bus_jobs",
        ))
        .await
        .unwrap();

        let mut hasher = XxHash3_64::with_seed(0);
        hasher.write(HandlerScheduled::HANDLER_IDENTITY.as_bytes());
        hasher.write(StateTestEvent::EVENT_IDENTITY.as_bytes());
        let hash_type_name = hasher.finish() as i64;

        let insert_now = Utc::now() - chrono::Duration::hours(10);

        let active_model = bus_jobs::ActiveModel {
            id: sea_orm::Set(Uuid::now_v7()),
            hash_type_name: sea_orm::Set(hash_type_name),
            inserted_at: sea_orm::Set(insert_now),
            scheduled_at: sea_orm::Set(insert_now),
            attempted_at: sea_orm::Set(None),
            completed_at: sea_orm::Set(None),
            cancelled_at: sea_orm::Set(None),
            discarded_at: sea_orm::Set(None),
            state: sea_orm::Set(BusJobState::Scheduled),
            priority: sea_orm::Set(10),
            attempt: sea_orm::Set(10),
            max_attempts: sea_orm::Set(10),
            execution_timeout_sec: sea_orm::Set(100),
            queue: sea_orm::Set("default".to_string()),
            type_name_event: sea_orm::Set(StateTestEvent::EVENT_IDENTITY.to_string()),
            type_name_handler: sea_orm::Set(HandlerScheduled::HANDLER_IDENTITY.to_string()),
            payload: sea_orm::Set(
                serde_json::to_value(StateTestEvent {
                    code: "UNIQUE_101".to_string(),
                })
                .expect("Should be serializable"),
            ),
            meta: sea_orm::Set(
                serde_json::to_value(BusMetadata::default()).expect("Should be serializable"),
            ),
            tags: sea_orm::Set(serde_json::json!(["1", "2", "3"])),
            errors: sea_orm::Set(serde_json::json!([])),
            attempted_by: sea_orm::Set(serde_json::json!([])),
        };

        active_model.insert(&txn).await.unwrap();

        let event = StateTestEvent {
            code: "UNIQUE_101".to_string(),
        };

        let mut meta = BusMetadata::default();
        meta.add("attempt", 1).unwrap();
        meta.add("request_id", Uuid::now_v7()).unwrap();
        meta.add("correlation_id", Uuid::now_v7()).unwrap();
        meta.add("causation_id", Uuid::now_v7()).unwrap();
        meta.add("flatten_val", Uuid::now_v7()).unwrap();

        #[cfg(feature = "context")]
        bus::event(&ExampleBusContext::new(&txn, meta), event.clone())
            .await
            .unwrap();

        #[cfg(not(feature = "context"))]
        bus::event(&txn, event.clone(), &meta).await.unwrap();

        let job = bus_jobs::Entity::find()
            .filter(bus_jobs::Column::TypeNameHandler.eq(HandlerScheduled::HANDLER_IDENTITY))
            .filter(bus_jobs::Column::TypeNameEvent.eq(StateTestEvent::EVENT_IDENTITY))
            .one(&txn)
            .await
            .expect("Job not found")
            .unwrap();

        assert_eq!(job.queue, "default".to_string());
        assert_eq!(job.priority, 21);
        assert_eq!(job.max_attempts, 100);
        assert_eq!(job.execution_timeout_sec, 333);
        assert_eq!(job.tags, serde_json::json!(["11", "22", "33"]));
        assert_eq!(job.meta, serde_json::to_value(meta).unwrap());

        txn.rollback().await.unwrap();
    }

    #[derive(Default)]
    pub struct HandlerExecuting;
    #[BusEventHandlerDatabase]
    impl IEventHandlerDatabase<StateTestEvent> for HandlerExecuting {
        const QUEUE: &'static str = "default";
        const PRIORITY: u32 = 21;
        const MAX_ATTEMPTS: Option<u32> = Some(100);
        const EXECUTION_TIMEOUT: Option<chrono::Duration> = Some(chrono::Duration::seconds(333));
        const SCHEDULE_IN: ScheduleIn = ScheduleIn::Immediately;
        const TAGS: &'static [&'static str] = &["11", "22", "33"];

        const UNIQUE: Option<Unique> = Some(Unique {
            period: Period::Infinite,
            fields: &[Field::Priority],
            keys: &["key_1", "key_2"],
            states: &[State::Executing],
            replace: &[Replace::Executing(&[
                ExecutingField::MaxAttempts,
                ExecutingField::Tags,
                ExecutingField::Meta,
            ])],
        });

        async fn handle(
            &self,
            _db: &DatabaseConnection,
            _event: &StateTestEvent,
            _metadata: &BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_unique_executing_replace() {
        setup_bus().await;
        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let txn = pool.begin().await.unwrap();

        txn.execute_raw(Statement::from_string(
            txn.get_database_backend(),
            "DELETE FROM bus_jobs",
        ))
        .await
        .unwrap();

        let mut hasher = XxHash3_64::with_seed(0);
        hasher.write(HandlerExecuting::HANDLER_IDENTITY.as_bytes());
        hasher.write(StateTestEvent::EVENT_IDENTITY.as_bytes());
        let hash_type_name = hasher.finish() as i64;

        let insert_now = Utc::now() - chrono::Duration::hours(10);

        let active_model = bus_jobs::ActiveModel {
            id: sea_orm::Set(Uuid::now_v7()),
            hash_type_name: sea_orm::Set(hash_type_name),
            inserted_at: sea_orm::Set(insert_now),
            scheduled_at: sea_orm::Set(insert_now),
            attempted_at: sea_orm::Set(None),
            completed_at: sea_orm::Set(None),
            cancelled_at: sea_orm::Set(None),
            discarded_at: sea_orm::Set(None),
            state: sea_orm::Set(BusJobState::Executing),
            priority: sea_orm::Set(21),
            attempt: sea_orm::Set(10),
            max_attempts: sea_orm::Set(10),
            execution_timeout_sec: sea_orm::Set(100),
            queue: sea_orm::Set("default".to_string()),
            type_name_event: sea_orm::Set(StateTestEvent::EVENT_IDENTITY.to_string()),
            type_name_handler: sea_orm::Set(HandlerExecuting::HANDLER_IDENTITY.to_string()),
            payload: sea_orm::Set(
                serde_json::to_value(StateTestEvent {
                    code: "UNIQUE_101".to_string(),
                })
                .expect("Should be serializable"),
            ),
            meta: sea_orm::Set(
                serde_json::to_value(BusMetadata::default()).expect("Should be serializable"),
            ),
            tags: sea_orm::Set(serde_json::json!(["1", "2", "3"])),
            errors: sea_orm::Set(serde_json::json!([])),
            attempted_by: sea_orm::Set(serde_json::json!([])),
        };

        active_model.insert(&txn).await.unwrap();

        let event = StateTestEvent {
            code: "UNIQUE_101".to_string(),
        };

        let mut meta = BusMetadata::default();
        meta.add("attempt", 1).unwrap();
        meta.add("request_id", Uuid::now_v7()).unwrap();
        meta.add("correlation_id", Uuid::now_v7()).unwrap();
        meta.add("causation_id", Uuid::now_v7()).unwrap();
        meta.add("flatten_val", Uuid::now_v7()).unwrap();

        #[cfg(feature = "context")]
        bus::event(&ExampleBusContext::new(&txn, meta), event.clone())
            .await
            .unwrap();

        #[cfg(not(feature = "context"))]
        bus::event(&txn, event.clone(), &meta).await.unwrap();

        let job = bus_jobs::Entity::find()
            .filter(bus_jobs::Column::TypeNameHandler.eq(HandlerExecuting::HANDLER_IDENTITY))
            .filter(bus_jobs::Column::TypeNameEvent.eq(StateTestEvent::EVENT_IDENTITY))
            .one(&txn)
            .await
            .expect("Job not found")
            .unwrap();

        assert_eq!(job.max_attempts, 100);
        assert_eq!(job.tags, serde_json::json!(["11", "22", "33"]));
        assert_eq!(job.meta, serde_json::to_value(meta).unwrap());

        txn.rollback().await.unwrap();
    }

    #[derive(Default)]
    pub struct HandlerRetryable;
    #[BusEventHandlerDatabase]
    impl IEventHandlerDatabase<StateTestEvent> for HandlerRetryable {
        const QUEUE: &'static str = "default";
        const PRIORITY: u32 = 21;
        const MAX_ATTEMPTS: Option<u32> = Some(100);
        const EXECUTION_TIMEOUT: Option<chrono::Duration> = Some(chrono::Duration::seconds(333));
        const SCHEDULE_IN: ScheduleIn = ScheduleIn::Immediately;
        const TAGS: &'static [&'static str] = &["11", "22", "33"];

        const UNIQUE: Option<Unique> = Some(Unique {
            period: Period::Infinite,
            fields: &[Field::MaxAttempts],
            keys: &["key_1", "key_2"],
            states: &[State::Retryable],
            replace: &[Replace::Retryable(&[
                RetryableField::Event,
                RetryableField::ExecutionTimeout,
                RetryableField::Tags,
                RetryableField::Meta,
            ])],
        });

        async fn handle(
            &self,
            _db: &DatabaseConnection,
            _event: &StateTestEvent,
            _metadata: &BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_unique_retryable_replace() {
        setup_bus().await;
        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let txn = pool.begin().await.unwrap();

        txn.execute_raw(Statement::from_string(
            txn.get_database_backend(),
            "DELETE FROM bus_jobs",
        ))
        .await
        .unwrap();

        let mut hasher = XxHash3_64::with_seed(0);
        hasher.write(HandlerRetryable::HANDLER_IDENTITY.as_bytes());
        hasher.write(StateTestEvent::EVENT_IDENTITY.as_bytes());
        let hash_type_name = hasher.finish() as i64;

        let insert_now = Utc::now() - chrono::Duration::hours(10);

        let active_model = bus_jobs::ActiveModel {
            id: sea_orm::Set(Uuid::now_v7()),
            hash_type_name: sea_orm::Set(hash_type_name),
            inserted_at: sea_orm::Set(insert_now),
            scheduled_at: sea_orm::Set(insert_now),
            attempted_at: sea_orm::Set(None),
            completed_at: sea_orm::Set(None),
            cancelled_at: sea_orm::Set(None),
            discarded_at: sea_orm::Set(None),
            state: sea_orm::Set(BusJobState::Retryable),
            priority: sea_orm::Set(10),
            attempt: sea_orm::Set(10),
            max_attempts: sea_orm::Set(100),
            execution_timeout_sec: sea_orm::Set(100),
            queue: sea_orm::Set("NOT_DEFAULT".to_string()),
            type_name_event: sea_orm::Set(StateTestEvent::EVENT_IDENTITY.to_string()),
            type_name_handler: sea_orm::Set(HandlerRetryable::HANDLER_IDENTITY.to_string()),
            payload: sea_orm::Set(
                serde_json::to_value(StateTestEvent {
                    code: "UNIQUE_101".to_string(),
                })
                .expect("Should be serializable"),
            ),
            meta: sea_orm::Set(
                serde_json::to_value(BusMetadata::default()).expect("Should be serializable"),
            ),
            tags: sea_orm::Set(serde_json::json!(["1", "2", "3"])),
            errors: sea_orm::Set(serde_json::json!([])),
            attempted_by: sea_orm::Set(serde_json::json!([])),
        };

        active_model.insert(&txn).await.unwrap();

        let event = StateTestEvent {
            code: "UNIQUE_102".to_string(),
        };

        let mut meta = BusMetadata::default();
        meta.add("attempt", 1).unwrap();
        meta.add("request_id", Uuid::now_v7()).unwrap();
        meta.add("correlation_id", Uuid::now_v7()).unwrap();
        meta.add("causation_id", Uuid::now_v7()).unwrap();
        meta.add("flatten_val", Uuid::now_v7()).unwrap();

        #[cfg(feature = "context")]
        bus::event(&ExampleBusContext::new(&txn, meta), event.clone())
            .await
            .unwrap();

        #[cfg(not(feature = "context"))]
        bus::event(&txn, event.clone(), &meta).await.unwrap();

        let job = bus_jobs::Entity::find()
            .filter(bus_jobs::Column::TypeNameHandler.eq(HandlerRetryable::HANDLER_IDENTITY))
            .filter(bus_jobs::Column::TypeNameEvent.eq(StateTestEvent::EVENT_IDENTITY))
            .one(&txn)
            .await
            .expect("Job not found")
            .unwrap();

        assert_eq!(job.execution_timeout_sec, 333);
        assert_eq!(job.tags, serde_json::json!(["11", "22", "33"]));
        assert_eq!(job.meta, serde_json::to_value(meta).unwrap());
        assert_eq!(job.payload, serde_json::to_value(event).unwrap());

        txn.rollback().await.unwrap();
    }

    #[derive(Default)]
    pub struct HandlerCompleted;
    #[BusEventHandlerDatabase]
    impl IEventHandlerDatabase<StateTestEvent> for HandlerCompleted {
        const QUEUE: &'static str = "default";
        const PRIORITY: u32 = 21;
        const MAX_ATTEMPTS: Option<u32> = Some(100);
        const EXECUTION_TIMEOUT: Option<chrono::Duration> = Some(chrono::Duration::seconds(333));
        const SCHEDULE_IN: ScheduleIn = ScheduleIn::Immediately;
        const TAGS: &'static [&'static str] = &["11", "22", "33"];

        const UNIQUE: Option<Unique> = Some(Unique {
            period: Period::Infinite,
            fields: &[],
            keys: &["key_1", "key_2"],
            states: &[State::Completed],
            replace: &[Replace::Completed(&[
                TerminalField::Tags,
                TerminalField::Meta,
            ])],
        });

        async fn handle(
            &self,
            _db: &DatabaseConnection,
            _event: &StateTestEvent,
            _metadata: &BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_unique_completed_replace() {
        setup_bus().await;
        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let txn = pool.begin().await.unwrap();

        txn.execute_raw(Statement::from_string(
            txn.get_database_backend(),
            "DELETE FROM bus_jobs",
        ))
        .await
        .unwrap();

        let mut hasher = XxHash3_64::with_seed(0);
        hasher.write(HandlerCompleted::HANDLER_IDENTITY.as_bytes());
        hasher.write(StateTestEvent::EVENT_IDENTITY.as_bytes());
        let hash_type_name = hasher.finish() as i64;

        let insert_now = Utc::now() - chrono::Duration::hours(10);

        let mut insert_meta = BusMetadata::default();
        insert_meta.add("custom_key", 1).unwrap();

        let active_model = bus_jobs::ActiveModel {
            id: sea_orm::Set(Uuid::now_v7()),
            hash_type_name: sea_orm::Set(hash_type_name),
            inserted_at: sea_orm::Set(insert_now),
            scheduled_at: sea_orm::Set(insert_now),
            attempted_at: sea_orm::Set(None),
            completed_at: sea_orm::Set(None),
            cancelled_at: sea_orm::Set(None),
            discarded_at: sea_orm::Set(None),
            state: sea_orm::Set(BusJobState::Completed),
            priority: sea_orm::Set(10),
            attempt: sea_orm::Set(10),
            max_attempts: sea_orm::Set(10),
            execution_timeout_sec: sea_orm::Set(100),
            queue: sea_orm::Set("NOT_DEFAULT".to_string()),
            type_name_event: sea_orm::Set(StateTestEvent::EVENT_IDENTITY.to_string()),
            type_name_handler: sea_orm::Set(HandlerCompleted::HANDLER_IDENTITY.to_string()),
            payload: sea_orm::Set(
                serde_json::to_value(StateTestEvent {
                    code: "UNIQUE_101".to_string(),
                })
                .expect("Should be serializable"),
            ),
            meta: sea_orm::Set(
                serde_json::to_value(BusMetadata::default()).expect("Should be serializable"),
            ),
            tags: sea_orm::Set(serde_json::json!(["1", "2", "3"])),
            errors: sea_orm::Set(serde_json::json!([])),
            attempted_by: sea_orm::Set(serde_json::json!([])),
        };

        active_model.insert(&txn).await.unwrap();

        let event = StateTestEvent {
            code: "UNIQUE_101".to_string(),
        };

        let mut meta = BusMetadata::default();
        meta.add("attempt", 1).unwrap();
        meta.add("request_id", Uuid::now_v7()).unwrap();
        meta.add("correlation_id", Uuid::now_v7()).unwrap();
        meta.add("causation_id", Uuid::now_v7()).unwrap();
        meta.add("flatten_val", Uuid::now_v7()).unwrap();

        #[cfg(feature = "context")]
        bus::event(&ExampleBusContext::new(&txn, meta), event.clone())
            .await
            .unwrap();

        #[cfg(not(feature = "context"))]
        bus::event(&txn, event.clone(), &meta).await.unwrap();

        let job = bus_jobs::Entity::find()
            .filter(bus_jobs::Column::TypeNameHandler.eq(HandlerCompleted::HANDLER_IDENTITY))
            .filter(bus_jobs::Column::TypeNameEvent.eq(StateTestEvent::EVENT_IDENTITY))
            .one(&txn)
            .await
            .expect("Job not found")
            .unwrap();

        assert_eq!(job.tags, serde_json::json!(["11", "22", "33"]));
        assert_eq!(job.meta, serde_json::to_value(meta).unwrap());

        txn.rollback().await.unwrap();
    }

    #[derive(Default)]
    pub struct HandlerCancelled;
    #[BusEventHandlerDatabase]
    impl IEventHandlerDatabase<StateTestEvent> for HandlerCancelled {
        const QUEUE: &'static str = "default";
        const PRIORITY: u32 = 21;
        const MAX_ATTEMPTS: Option<u32> = Some(100);
        const EXECUTION_TIMEOUT: Option<chrono::Duration> = Some(chrono::Duration::seconds(333));
        const SCHEDULE_IN: ScheduleIn = ScheduleIn::Immediately;
        const TAGS: &'static [&'static str] = &["11", "22", "33"];

        const UNIQUE: Option<Unique> = Some(Unique {
            period: Period::Infinite,
            fields: &[],
            keys: &["key_1", "key_2"],
            states: &[State::Cancelled],
            replace: &[Replace::Cancelled(&[
                TerminalField::Tags,
                TerminalField::Meta,
            ])],
        });

        async fn handle(
            &self,
            _db: &DatabaseConnection,
            _event: &StateTestEvent,
            _metadata: &BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_unique_cancelled_replace() {
        setup_bus().await;
        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let txn = pool.begin().await.unwrap();

        txn.execute_raw(Statement::from_string(
            txn.get_database_backend(),
            "DELETE FROM bus_jobs",
        ))
        .await
        .unwrap();

        let mut hasher = XxHash3_64::with_seed(0);
        hasher.write(HandlerCancelled::HANDLER_IDENTITY.as_bytes());
        hasher.write(StateTestEvent::EVENT_IDENTITY.as_bytes());
        let hash_type_name = hasher.finish() as i64;

        let insert_now = Utc::now() - chrono::Duration::hours(10);

        let mut insert_meta = BusMetadata::default();
        insert_meta.add("custom_key", 1).unwrap();

        let active_model = bus_jobs::ActiveModel {
            id: sea_orm::Set(Uuid::now_v7()),
            hash_type_name: sea_orm::Set(hash_type_name),
            inserted_at: sea_orm::Set(insert_now),
            scheduled_at: sea_orm::Set(insert_now),
            attempted_at: sea_orm::Set(None),
            completed_at: sea_orm::Set(None),
            cancelled_at: sea_orm::Set(None),
            discarded_at: sea_orm::Set(None),
            state: sea_orm::Set(BusJobState::Cancelled),
            priority: sea_orm::Set(10),
            attempt: sea_orm::Set(10),
            max_attempts: sea_orm::Set(10),
            execution_timeout_sec: sea_orm::Set(100),
            queue: sea_orm::Set("NOT_DEFAULT".to_string()),
            type_name_event: sea_orm::Set(StateTestEvent::EVENT_IDENTITY.to_string()),
            type_name_handler: sea_orm::Set(HandlerCancelled::HANDLER_IDENTITY.to_string()),
            payload: sea_orm::Set(
                serde_json::to_value(StateTestEvent {
                    code: "UNIQUE_101".to_string(),
                })
                .expect("Should be serializable"),
            ),
            meta: sea_orm::Set(
                serde_json::to_value(BusMetadata::default()).expect("Should be serializable"),
            ),
            tags: sea_orm::Set(serde_json::json!(["1", "2", "3"])),
            errors: sea_orm::Set(serde_json::json!([])),
            attempted_by: sea_orm::Set(serde_json::json!([])),
        };

        active_model.insert(&txn).await.unwrap();

        let event = StateTestEvent {
            code: "UNIQUE_101".to_string(),
        };

        let mut meta = BusMetadata::default();
        meta.add("attempt", 1).unwrap();
        meta.add("request_id", Uuid::now_v7()).unwrap();
        meta.add("correlation_id", Uuid::now_v7()).unwrap();
        meta.add("causation_id", Uuid::now_v7()).unwrap();
        meta.add("flatten_val", Uuid::now_v7()).unwrap();

        #[cfg(feature = "context")]
        bus::event(&ExampleBusContext::new(&txn, meta), event.clone())
            .await
            .unwrap();

        #[cfg(not(feature = "context"))]
        bus::event(&txn, event.clone(), &meta).await.unwrap();

        let job = bus_jobs::Entity::find()
            .filter(bus_jobs::Column::TypeNameHandler.eq(HandlerCancelled::HANDLER_IDENTITY))
            .filter(bus_jobs::Column::TypeNameEvent.eq(StateTestEvent::EVENT_IDENTITY))
            .one(&txn)
            .await
            .expect("Job not found")
            .unwrap();

        assert_eq!(job.tags, serde_json::json!(["11", "22", "33"]));
        assert_eq!(job.meta, serde_json::to_value(meta).unwrap());

        txn.rollback().await.unwrap();
    }

    #[derive(Default)]
    pub struct HandlerDiscarded;
    #[BusEventHandlerDatabase]
    impl IEventHandlerDatabase<StateTestEvent> for HandlerDiscarded {
        const QUEUE: &'static str = "default";
        const PRIORITY: u32 = 21;
        const MAX_ATTEMPTS: Option<u32> = Some(100);
        const EXECUTION_TIMEOUT: Option<chrono::Duration> = Some(chrono::Duration::seconds(333));
        const SCHEDULE_IN: ScheduleIn = ScheduleIn::Immediately;
        const TAGS: &'static [&'static str] = &["11", "22", "33"];

        const UNIQUE: Option<Unique> = Some(Unique {
            period: Period::Infinite,
            fields: &[],
            keys: &["key_1", "key_2"],
            states: &[State::Discarded],
            replace: &[Replace::Discarded(&[
                TerminalField::Tags,
                TerminalField::Meta,
            ])],
        });

        async fn handle(
            &self,
            _db: &DatabaseConnection,
            _event: &StateTestEvent,
            _metadata: &BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_unique_discarded_replace() {
        setup_bus().await;
        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let txn = pool.begin().await.unwrap();

        txn.execute_raw(Statement::from_string(
            txn.get_database_backend(),
            "DELETE FROM bus_jobs",
        ))
        .await
        .unwrap();

        let mut hasher = XxHash3_64::with_seed(0);
        hasher.write(HandlerDiscarded::HANDLER_IDENTITY.as_bytes());
        hasher.write(StateTestEvent::EVENT_IDENTITY.as_bytes());
        let hash_type_name = hasher.finish() as i64;

        let insert_now = Utc::now() - chrono::Duration::hours(10);

        let mut insert_meta = BusMetadata::default();
        insert_meta.add("custom_key", 1).unwrap();

        let active_model = bus_jobs::ActiveModel {
            id: sea_orm::Set(Uuid::now_v7()),
            hash_type_name: sea_orm::Set(hash_type_name),
            inserted_at: sea_orm::Set(insert_now),
            scheduled_at: sea_orm::Set(insert_now),
            attempted_at: sea_orm::Set(None),
            completed_at: sea_orm::Set(None),
            cancelled_at: sea_orm::Set(None),
            discarded_at: sea_orm::Set(None),
            state: sea_orm::Set(BusJobState::Discarded),
            priority: sea_orm::Set(10),
            attempt: sea_orm::Set(10),
            max_attempts: sea_orm::Set(10),
            execution_timeout_sec: sea_orm::Set(100),
            queue: sea_orm::Set("NOT_DEFAULT".to_string()),
            type_name_event: sea_orm::Set(StateTestEvent::EVENT_IDENTITY.to_string()),
            type_name_handler: sea_orm::Set(HandlerDiscarded::HANDLER_IDENTITY.to_string()),
            payload: sea_orm::Set(
                serde_json::to_value(StateTestEvent {
                    code: "UNIQUE_101".to_string(),
                })
                .expect("Should be serializable"),
            ),
            meta: sea_orm::Set(
                serde_json::to_value(BusMetadata::default()).expect("Should be serializable"),
            ),
            tags: sea_orm::Set(serde_json::json!(["1", "2", "3"])),
            errors: sea_orm::Set(serde_json::json!([])),
            attempted_by: sea_orm::Set(serde_json::json!([])),
        };

        active_model.insert(&txn).await.unwrap();

        let event = StateTestEvent {
            code: "UNIQUE_101".to_string(),
        };

        let mut meta = BusMetadata::default();
        meta.add("attempt", 1).unwrap();
        meta.add("request_id", Uuid::now_v7()).unwrap();
        meta.add("correlation_id", Uuid::now_v7()).unwrap();
        meta.add("causation_id", Uuid::now_v7()).unwrap();
        meta.add("flatten_val", Uuid::now_v7()).unwrap();

        #[cfg(feature = "context")]
        bus::event(&ExampleBusContext::new(&txn, meta), event.clone())
            .await
            .unwrap();

        #[cfg(not(feature = "context"))]
        bus::event(&txn, event.clone(), &meta).await.unwrap();

        let job = bus_jobs::Entity::find()
            .filter(bus_jobs::Column::TypeNameHandler.eq(HandlerDiscarded::HANDLER_IDENTITY))
            .filter(bus_jobs::Column::TypeNameEvent.eq(StateTestEvent::EVENT_IDENTITY))
            .one(&txn)
            .await
            .expect("Job not found")
            .unwrap();

        assert_eq!(job.tags, serde_json::json!(["11", "22", "33"]));
        assert_eq!(job.meta, serde_json::to_value(meta).unwrap());

        txn.rollback().await.unwrap();
    }

    #[derive(Default)]
    pub struct HandlerNoReplace;
    #[BusEventHandlerDatabase]
    impl IEventHandlerDatabase<StateTestEvent> for HandlerNoReplace {
        const QUEUE: &'static str = "default";
        const PRIORITY: u32 = 21;
        const MAX_ATTEMPTS: Option<u32> = Some(100);
        const EXECUTION_TIMEOUT: Option<chrono::Duration> = Some(chrono::Duration::seconds(333));
        const SCHEDULE_IN: ScheduleIn = ScheduleIn::Immediately;
        const TAGS: &'static [&'static str] = &["11", "22", "33"];

        const UNIQUE: Option<Unique> = Some(Unique {
            period: Period::Infinite,
            fields: &[Field::Event],
            keys: &["key_1", "key_2"],
            states: &[State::Available],
            replace: &[],
        });

        async fn handle(
            &self,
            _db: &DatabaseConnection,
            _event: &StateTestEvent,
            _metadata: &BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_unique_no_replace() {
        setup_bus().await;
        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let txn = pool.begin().await.unwrap();

        txn.execute_raw(Statement::from_string(
            txn.get_database_backend(),
            "DELETE FROM bus_jobs",
        ))
        .await
        .unwrap();

        let mut hasher = XxHash3_64::with_seed(0);
        hasher.write(HandlerNoReplace::HANDLER_IDENTITY.as_bytes());
        hasher.write(StateTestEvent::EVENT_IDENTITY.as_bytes());
        let hash_type_name = hasher.finish() as i64;

        let insert_now = Utc::now() - chrono::Duration::hours(10);

        let active_model = bus_jobs::ActiveModel {
            id: sea_orm::Set(Uuid::now_v7()),
            hash_type_name: sea_orm::Set(hash_type_name),
            inserted_at: sea_orm::Set(insert_now),
            scheduled_at: sea_orm::Set(insert_now),
            attempted_at: sea_orm::Set(None),
            completed_at: sea_orm::Set(None),
            cancelled_at: sea_orm::Set(None),
            discarded_at: sea_orm::Set(None),
            state: sea_orm::Set(BusJobState::Available),
            priority: sea_orm::Set(10),
            attempt: sea_orm::Set(10),
            max_attempts: sea_orm::Set(10),
            execution_timeout_sec: sea_orm::Set(100),
            queue: sea_orm::Set("NOT_DEFAULT".to_string()),
            type_name_event: sea_orm::Set(StateTestEvent::EVENT_IDENTITY.to_string()),
            type_name_handler: sea_orm::Set(HandlerNoReplace::HANDLER_IDENTITY.to_string()),
            payload: sea_orm::Set(
                serde_json::to_value(StateTestEvent {
                    code: "UNIQUE_101".to_string(),
                })
                .expect("Should be serializable"),
            ),
            meta: sea_orm::Set(
                serde_json::to_value(BusMetadata::default()).expect("Should be serializable"),
            ),
            tags: sea_orm::Set(serde_json::json!(["1", "2", "3"])),
            errors: sea_orm::Set(serde_json::json!([])),
            attempted_by: sea_orm::Set(serde_json::json!([])),
        };

        active_model.insert(&txn).await.unwrap();

        let event = StateTestEvent {
            code: "UNIQUE_101".to_string(),
        };

        let mut meta = BusMetadata::default();
        meta.add("attempt", 1).unwrap();
        meta.add("request_id", Uuid::now_v7()).unwrap();
        meta.add("correlation_id", Uuid::now_v7()).unwrap();
        meta.add("causation_id", Uuid::now_v7()).unwrap();
        meta.add("flatten_val", Uuid::now_v7()).unwrap();

        #[cfg(feature = "context")]
        bus::event(&ExampleBusContext::new(&txn, meta), event.clone())
            .await
            .unwrap();

        #[cfg(not(feature = "context"))]
        bus::event(&txn, event.clone(), &meta).await.unwrap();

        let job = bus_jobs::Entity::find()
            .filter(bus_jobs::Column::TypeNameHandler.eq(HandlerNoReplace::HANDLER_IDENTITY))
            .filter(bus_jobs::Column::TypeNameEvent.eq(StateTestEvent::EVENT_IDENTITY))
            .one(&txn)
            .await
            .expect("Job not found")
            .unwrap();

        assert_eq!(job.queue, "NOT_DEFAULT".to_string());
        assert_eq!(job.priority, 10);
        assert_eq!(job.max_attempts, 10);
        assert_eq!(job.execution_timeout_sec, 100);
        assert_eq!(job.tags, serde_json::json!(["1", "2", "3"]));
        assert_eq!(
            job.meta,
            serde_json::to_value(BusMetadata::default()).unwrap()
        );

        use sea_orm::PaginatorTrait;
        let count = bus_jobs::Entity::find()
            .filter(bus_jobs::Column::TypeNameHandler.eq(HandlerNoReplace::HANDLER_IDENTITY))
            .count(&txn)
            .await
            .unwrap();

        assert_eq!(count, 1);

        txn.rollback().await.unwrap();
    }

    #[derive(Default)]
    pub struct TimeWindowHandler;

    #[BusEventHandlerDatabase]
    impl IEventHandlerDatabase<StateTestEvent> for TimeWindowHandler {
        const QUEUE: &'static str = "default";

        const UNIQUE: Option<Unique> = Some(Unique {
            period: Period::Duration {
                field: Timestamp::InsertedAt,
                duration: std::time::Duration::from_secs(10),
            },
            fields: &[Field::Event],
            keys: &[],
            states: &[State::Available],
            replace: &[],
        });

        async fn handle(
            &self,
            _db: &sea_orm::DatabaseConnection,
            _event: &StateTestEvent,
            _metadata: &crate::contracts::meta::BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_unique_period_duration_expiration() {
        setup_bus().await;
        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let txn = pool.begin().await.unwrap();

        txn.execute_raw(Statement::from_string(
            txn.get_database_backend(),
            "DELETE FROM bus_jobs",
        ))
        .await
        .unwrap();

        let event = StateTestEvent {
            code: "TIME_808".to_string(),
        };

        #[cfg(feature = "context")]
        let ctx = &ExampleBusContext::new(&txn, meta);
        #[cfg(feature = "context")]
        bus::event(&ctx, event.clone()).await.unwrap();

        #[cfg(not(feature = "context"))]
        bus::event(&txn, event.clone(), &BusMetadata::default())
            .await
            .unwrap();

        #[cfg(feature = "sea-orm-mysql")]
        let interval_expr = Expr::cust("now() - INTERVAL 1 HOUR");
        #[cfg(feature = "sea-orm-postgres")]
        let interval_expr = Expr::cust("now() - interval '1 hour'");

        bus_jobs::Entity::update_many()
            .col_expr(bus_jobs::Column::InsertedAt, interval_expr)
            .filter(bus_jobs::Column::TypeNameHandler.eq(TimeWindowHandler::HANDLER_IDENTITY))
            .exec(&txn)
            .await
            .unwrap();

        #[cfg(feature = "context")]
        bus::event(&ctx, event.clone()).await.unwrap();

        #[cfg(not(feature = "context"))]
        bus::event(&txn, event.clone(), &BusMetadata::default())
            .await
            .unwrap();

        let count = bus_jobs::Entity::find()
            .filter(bus_jobs::Column::TypeNameHandler.eq(TimeWindowHandler::HANDLER_IDENTITY))
            .count(&txn)
            .await
            .unwrap();

        assert_eq!(
            count, 2,
            "Should create a new row because the old one is outside the 10s window"
        );

        #[cfg(feature = "context")]
        bus::event(&ctx, event.clone()).await.unwrap();

        #[cfg(not(feature = "context"))]
        bus::event(&txn, event.clone(), &BusMetadata::default())
            .await
            .unwrap();

        let count_final = bus_jobs::Entity::find()
            .filter(bus_jobs::Column::TypeNameHandler.eq(TimeWindowHandler::HANDLER_IDENTITY))
            .count(&txn)
            .await
            .unwrap();

        assert_eq!(
            count_final, 2,
            "Should NOT create a third row because the second one is still fresh"
        );

        txn.rollback().await.unwrap();
    }
}
