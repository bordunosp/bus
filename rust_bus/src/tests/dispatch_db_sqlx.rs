#[cfg(test)]
mod sqlx_integration_tests {
    use crate as rust_bus;
    use crate::bus;
    use crate::contracts::database_unique::Unique;
    use crate::contracts::enums::{
        AvailableField, Field, Period, Replace, ScheduleIn, ScheduledField, TerminalField,
    };
    use crate::contracts::enums::{ExecutingField, RetryableField, State, Timestamp};
    use crate::contracts::event_handler_database::IEventHandlerDatabase;
    use crate::contracts::meta::BusMetadata;
    use crate::tests::base::setup_bus;
    use crate::workers::configuration::BusQueueConfiguration;
    use chrono::Utc;
    use rust_bus::{BusEvent, BusEventHandlerDatabase};
    use serde_json::Value;
    use sqlx::Row;
    use std::hash::Hasher;
    use twox_hash::XxHash3_64;
    use uuid::Uuid;

    const INSERT_SQL: &str = "INSERT INTO bus_jobs (id, hash_type_name, inserted_at, scheduled_at, attempted_at, completed_at, cancelled_at, discarded_at, state, priority, attempt, max_attempts, execution_timeout_sec, queue, type_name_event, type_name_handler, payload, meta, tags, errors, attempted_by) ";

    #[cfg(feature = "sqlx-mysql")]
    const SELECT_JOB_SQL: &str =
        "SELECT * FROM bus_jobs WHERE type_name_handler = ? AND type_name_event = ? LIMIT 1";

    #[cfg(feature = "sqlx-postgres")]
    const SELECT_JOB_SQL: &str = "SELECT *, state::text FROM bus_jobs WHERE type_name_handler = $1 AND type_name_event = $2 LIMIT 1";

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

    use internal::OrderCancelled as CancelledEvent;

    #[tokio::test]
    async fn test_event_identity_with_alias() {
        setup_bus().await;
        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let mut txn = pool.begin().await.unwrap();

        sqlx::query("DELETE FROM bus_jobs")
            .execute(&mut *txn)
            .await
            .unwrap();

        let event = CancelledEvent { order_id: 777 };

        #[cfg(feature = "context")]
        let ctx = ExampleBusContext::new(&txn, BusMetadata::default());
        #[cfg(feature = "context")]
        bus::event(&mut ctx, event).await.unwrap();

        #[cfg(not(feature = "context"))]
        bus::event(&mut txn, event, &BusMetadata::default())
            .await
            .unwrap();

        #[cfg(feature = "sqlx-mysql")]
        let sql: &str = "SELECT * FROM bus_jobs WHERE type_name_handler = ? LIMIT 1";

        #[cfg(feature = "sqlx-postgres")]
        let sql: &str = "SELECT * FROM bus_jobs WHERE type_name_handler = $1 LIMIT 1";

        let row = sqlx::query(sql)
            .bind(OrderHandlerUseAs::HANDLER_IDENTITY)
            .fetch_one(&mut *txn)
            .await
            .unwrap();

        assert_eq!(
            row.get::<String, _>("type_name_handler"),
            "rust_bus::tests::dispatch_db_sqlx::sqlx_integration_tests::OrderHandlerUseAs"
        );
        assert_eq!(
            row.get::<String, _>("type_name_event"),
            "rust_bus::tests::dispatch_db_sqlx::sqlx_integration_tests::internal::OrderCancelled"
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

        #[cfg(feature = "sqlx-postgres")]
        async fn handle(
            &self,
            _db: &sqlx::PgPool,
            _event: &OrderCreated,
            _metadata: &crate::contracts::meta::BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }

        #[cfg(feature = "sqlx-mysql")]
        async fn handle(
            &self,
            _db: &sqlx::MySqlPool,
            _event: &OrderCreated,
            _metadata: &crate::contracts::meta::BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    #[derive(Default)]
    pub struct OrderHandlerDefault;

    #[BusEventHandlerDatabase]
    impl IEventHandlerDatabase<OrderCreated> for OrderHandlerDefault {
        const QUEUE: &'static str = "default";

        #[cfg(feature = "sqlx-postgres")]
        async fn handle(
            &self,
            _db: &sqlx::PgPool,
            _event: &OrderCreated,
            _metadata: &crate::contracts::meta::BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }

        #[cfg(feature = "sqlx-mysql")]
        async fn handle(
            &self,
            _db: &sqlx::MySqlPool,
            _event: &OrderCreated,
            _metadata: &crate::contracts::meta::BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    #[derive(Default)]
    pub struct OrderHandlerUseAs;

    #[BusEventHandlerDatabase]
    impl IEventHandlerDatabase<internal::OrderCancelled> for OrderHandlerUseAs {
        const QUEUE: &'static str = "default";

        #[cfg(feature = "sqlx-postgres")]
        async fn handle(
            &self,
            _db: &sqlx::PgPool,
            _event: &internal::OrderCancelled,
            _metadata: &crate::contracts::meta::BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }

        #[cfg(feature = "sqlx-mysql")]
        async fn handle(
            &self,
            _db: &sqlx::MySqlPool,
            _event: &internal::OrderCancelled,
            _metadata: &crate::contracts::meta::BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_save_to_db() {
        setup_bus().await;
        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let mut txn = pool.begin().await.unwrap();

        sqlx::query("DELETE FROM bus_jobs")
            .execute(&mut *txn)
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
        bus::event(&mut txn, event.clone(), &meta.clone())
            .await
            .unwrap();

        let row = sqlx::query(SELECT_JOB_SQL)
            .bind(OrderHandler::HANDLER_IDENTITY)
            .bind(OrderCreated::EVENT_IDENTITY)
            .fetch_one(&mut *txn)
            .await
            .unwrap();

        assert_eq!(
            row.get::<uuid::Uuid, _>("id").get_version(),
            Some(uuid::Version::SortRand),
            "Job ID must be a UUID v7 for proper sorting"
        );

        assert_eq!(
            row.get::<String, _>("type_name_handler"),
            "rust_bus::tests::dispatch_db_sqlx::sqlx_integration_tests::OrderHandler"
        );
        assert_eq!(
            row.get::<String, _>("type_name_event"),
            "rust_bus::tests::dispatch_db_sqlx::sqlx_integration_tests::OrderCreated"
        );
        assert_eq!(row.get::<String, _>("queue"), "default");

        assert_eq!(row.get::<String, _>("state"), "available".to_string());
        assert_eq!(row.get::<i32, _>("priority"), 10);
        assert_eq!(row.get::<i32, _>("attempt"), 0);
        assert_eq!(row.get::<i32, _>("max_attempts"), 22);
        assert_eq!(row.get::<i32, _>("execution_timeout_sec"), 14 * 60);

        let inserted_at: chrono::DateTime<Utc> = row.get::<chrono::DateTime<Utc>, _>("inserted_at");
        let scheduled_at: chrono::DateTime<Utc> =
            row.get::<chrono::DateTime<Utc>, _>("scheduled_at");
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
            "attempted_at",
            "completed_at",
            "discarded_at",
            "cancelled_at",
        ] {
            assert!(
                row.try_get::<chrono::DateTime<chrono::Utc>, _>(field_name)
                    .is_err()
                    || row
                        .get::<Option<chrono::DateTime<chrono::Utc>>, _>(field_name)
                        .is_none()
            );
        }

        let payload_json: serde_json::Value = row.get("payload");
        let meta_json: serde_json::Value = row.get("meta");
        let payload_from_json: OrderCreated = serde_json::from_value(payload_json).unwrap();
        let meta_from_json: BusMetadata = serde_json::from_value(meta_json).unwrap();

        assert_eq!(payload_from_json.id, 100);
        assert_eq!(meta_from_json.get("user_id"), Some(42));
        assert_eq!(
            meta_from_json.get("info"),
            Some("first_attempt".to_string())
        );
        assert_eq!(
            meta_from_json.get::<Uuid>("request_id"),
            meta.get::<Uuid>("request_id")
        );

        let tags_json: serde_json::Value =
            row.get::<sqlx::types::Json<serde_json::Value>, _>("tags").0;
        let tags_array = tags_json.as_array().expect("Tags should be an array");
        assert!(tags_array.iter().any(|t| t.as_str() == Some("user")));
        assert!(tags_array.iter().any(|t| t.as_str() == Some("company")));

        let errors: serde_json::Value = row
            .get::<sqlx::types::Json<serde_json::Value>, _>("errors")
            .0;
        let attempted_by: serde_json::Value = row
            .get::<sqlx::types::Json<serde_json::Value>, _>("attempted_by")
            .0;
        assert!(errors.as_array().unwrap().is_empty());
        assert!(attempted_by.as_array().unwrap().is_empty());

        {
            let row = sqlx::query(SELECT_JOB_SQL)
                .bind(OrderHandlerDefault::HANDLER_IDENTITY)
                .bind(OrderCreated::EVENT_IDENTITY)
                .fetch_one(&mut *txn)
                .await
                .unwrap();

            assert_eq!(row.get::<i32, _>("priority"), 0);
            assert_eq!(row.get::<i32, _>("max_attempts"), 3);
            assert_eq!(row.get::<i32, _>("execution_timeout_sec"), 10 * 60);
            assert!(row.get::<chrono::DateTime<Utc>, _>("inserted_at") <= Utc::now());

            let diff = row.get::<chrono::DateTime<Utc>, _>("scheduled_at") - Utc::now();
            assert!(
                diff.num_seconds().abs() < 5,
                "For default handler, scheduled_at must be equal to inserted_at"
            );
        }

        txn.rollback().await.unwrap();

        println!("✅ All fields verified successfully");
    }

    #[cfg(feature = "sqlx-mysql")]
    async fn insert_job(
        txn: &mut sqlx::Transaction<'_, sqlx::MySql>,
        state: &str,
        hash_type_name: i64,
        queue: &str,
        type_name_event: &str,
        type_name_handler: &str,
        event_json: Value,
        meta_json: Value,
    ) {
        let insert_now = Utc::now() - chrono::Duration::hours(10);
        let mut qb = sqlx::QueryBuilder::<sqlx::MySql>::new(INSERT_SQL);

        qb.push(" VALUES (");
        let mut separated = qb.separated(", ");

        separated.push_bind(uuid::Uuid::now_v7());
        separated.push_bind(hash_type_name);
        separated.push_bind(insert_now);
        separated.push_bind(insert_now);

        separated.push_bind(None::<chrono::DateTime<chrono::Utc>>); // attempted_at
        separated.push_bind(None::<chrono::DateTime<chrono::Utc>>); // completed_at
        separated.push_bind(None::<chrono::DateTime<chrono::Utc>>); // cancelled_at
        separated.push_bind(None::<chrono::DateTime<chrono::Utc>>); // discarded_at

        separated.push_bind(state);

        separated.push_bind(10i32); // priority
        separated.push_bind(10i32); // attempt
        separated.push_bind(10i32); // max_attempts
        separated.push_bind(100i32); // execution_timeout_sec

        separated.push_bind(queue);
        separated.push_bind(type_name_event);
        separated.push_bind(type_name_handler);

        separated.push_bind(sqlx::types::Json(event_json));
        separated.push_bind(sqlx::types::Json(meta_json));
        separated.push_bind(sqlx::types::Json(serde_json::json!(["1", "2", "3"])));
        separated.push_bind(sqlx::types::Json(serde_json::json!([])));
        separated.push_bind(sqlx::types::Json(serde_json::json!([])));

        qb.push(")");

        qb.build()
            .execute(&mut **txn)
            .await
            .expect("Failed to insert manual job (MySQL)");
    }

    #[cfg(feature = "sqlx-postgres")]
    async fn insert_job(
        txn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        state: &str,
        hash_type_name: i64,
        queue: &str,
        type_name_event: &str,
        type_name_handler: &str,
        event_json: Value,
        meta_json: Value,
    ) {
        let insert_now = Utc::now() - chrono::Duration::hours(10);
        let mut qb = sqlx::QueryBuilder::<sqlx::Postgres>::new(INSERT_SQL);

        qb.push(" VALUES (");

        // 1. id
        qb.push_bind(uuid::Uuid::now_v7());
        qb.push(", ");

        // 2. hash_type_name
        qb.push_bind(hash_type_name);
        qb.push(", ");

        // 3. inserted_at
        qb.push_bind(insert_now);
        qb.push(", ");

        // 4. scheduled_at
        qb.push_bind(insert_now);
        qb.push(", ");

        // 5-8. NULL dates (attempted, completed, cancelled, discarded)
        qb.push_bind(None::<chrono::DateTime<chrono::Utc>>);
        qb.push(", ");
        qb.push_bind(None::<chrono::DateTime<chrono::Utc>>);
        qb.push(", ");
        qb.push_bind(None::<chrono::DateTime<chrono::Utc>>);
        qb.push(", ");
        qb.push_bind(None::<chrono::DateTime<chrono::Utc>>);
        qb.push(", ");

        // 9. state (з кастом)
        qb.push_bind(state);
        qb.push("::bus_job_state, ");

        // 10. priority
        qb.push_bind(10i32);
        qb.push(", ");

        // 11. attempt
        qb.push_bind(10i32);
        qb.push(", ");

        // 12. max_attempts
        qb.push_bind(10i32);
        qb.push(", ");

        // 13. execution_timeout_sec
        qb.push_bind(100i32);
        qb.push(", ");

        // 14. queue
        qb.push_bind(queue);
        qb.push(", ");

        // 15. type_name_event
        qb.push_bind(type_name_event);
        qb.push(", ");

        // 16. type_name_handler
        qb.push_bind(type_name_handler);
        qb.push(", ");

        // 17. payload
        qb.push_bind(sqlx::types::Json(event_json));
        qb.push(", ");

        // 18. meta
        qb.push_bind(sqlx::types::Json(meta_json));
        qb.push(", ");

        // 19. tags
        qb.push_bind(sqlx::types::Json(serde_json::json!(["1", "2", "3"])));
        qb.push(", ");

        // 20. errors
        qb.push_bind(sqlx::types::Json(serde_json::json!([])));
        qb.push(", ");

        // 21. attempted_by
        qb.push_bind(sqlx::types::Json(serde_json::json!([])));

        qb.push(")");

        qb.build()
            .execute(&mut **txn)
            .await
            .expect("Failed to insert manual job (Postgres)");
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

        #[cfg(feature = "sqlx-postgres")]
        async fn handle(
            &self,
            _db: &sqlx::PgPool,
            _event: &StateTestEvent,
            _metadata: &crate::contracts::meta::BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }

        #[cfg(feature = "sqlx-mysql")]
        async fn handle(
            &self,
            _db: &sqlx::MySqlPool,
            _event: &StateTestEvent,
            _metadata: &crate::contracts::meta::BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_unique_available_replace() {
        setup_bus().await;
        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let mut txn = pool.begin().await.unwrap();

        sqlx::query("DELETE FROM bus_jobs")
            .execute(&mut *txn)
            .await
            .unwrap();

        let mut hasher = XxHash3_64::with_seed(0);
        hasher.write(HandlerAvailable::HANDLER_IDENTITY.as_bytes());
        hasher.write(StateTestEvent::EVENT_IDENTITY.as_bytes());
        let hash_type_name = hasher.finish() as i64;

        let event = StateTestEvent {
            code: "UNIQUE_101".to_string(),
        };

        insert_job(
            &mut txn,
            "available",
            hash_type_name,
            "NOT_DEFAULT",
            StateTestEvent::EVENT_IDENTITY,
            HandlerAvailable::HANDLER_IDENTITY,
            serde_json::to_value(&event).unwrap(),
            serde_json::to_value(&BusMetadata::default()).unwrap(),
        )
        .await;

        let mut meta = BusMetadata::default();
        meta.add("attempt", 1).unwrap();
        meta.add("request_id", Uuid::now_v7()).unwrap();
        meta.add("correlation_id", Uuid::now_v7()).unwrap();
        meta.add("causation_id", Uuid::now_v7()).unwrap();
        meta.add("flatten_val", Uuid::now_v7()).unwrap();

        #[cfg(feature = "context")]
        bus::event(
            &mut ExampleBusContext::new(&txn, meta.clone()),
            event.clone(),
        )
        .await
        .unwrap();

        #[cfg(not(feature = "context"))]
        bus::event(&mut txn, event.clone(), &meta).await.unwrap();

        let row = sqlx::query(SELECT_JOB_SQL)
            .bind(HandlerAvailable::HANDLER_IDENTITY)
            .bind(StateTestEvent::EVENT_IDENTITY)
            .fetch_one(&mut *txn)
            .await
            .unwrap();

        assert_eq!(row.get::<String, _>("queue"), "default".to_string());
        assert_eq!(row.get::<i32, _>("priority"), 21);
        assert_eq!(row.get::<i32, _>("max_attempts"), 100);
        assert_eq!(row.get::<i32, _>("execution_timeout_sec"), 333);

        let row_meta: serde_json::Value =
            row.get::<sqlx::types::Json<serde_json::Value>, _>("meta").0;
        assert_eq!(row_meta, serde_json::to_value(&meta).unwrap());

        let row_tags: serde_json::Value =
            row.get::<sqlx::types::Json<serde_json::Value>, _>("tags").0;
        assert_eq!(row_tags, serde_json::json!(["11", "22", "33"]));

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

        #[cfg(feature = "sqlx-postgres")]
        async fn handle(
            &self,
            _db: &sqlx::PgPool,
            _event: &StateTestEvent,
            _metadata: &crate::contracts::meta::BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }

        #[cfg(feature = "sqlx-mysql")]
        async fn handle(
            &self,
            _db: &sqlx::MySqlPool,
            _event: &StateTestEvent,
            _metadata: &crate::contracts::meta::BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_unique_scheduled_replace() {
        setup_bus().await;
        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let mut txn = pool.begin().await.unwrap();

        sqlx::query("DELETE FROM bus_jobs")
            .execute(&mut *txn)
            .await
            .unwrap();

        let mut hasher = XxHash3_64::with_seed(0);
        hasher.write(HandlerScheduled::HANDLER_IDENTITY.as_bytes());
        hasher.write(StateTestEvent::EVENT_IDENTITY.as_bytes());
        let hash_type_name = hasher.finish() as i64;

        let event = StateTestEvent {
            code: "UNIQUE_101".to_string(),
        };

        insert_job(
            &mut txn,
            "scheduled",
            hash_type_name,
            "default",
            StateTestEvent::EVENT_IDENTITY,
            HandlerScheduled::HANDLER_IDENTITY,
            serde_json::to_value(&event).unwrap(),
            serde_json::to_value(&BusMetadata::default()).unwrap(),
        )
        .await;

        let mut meta = BusMetadata::default();
        meta.add("attempt", 1).unwrap();
        meta.add("request_id", Uuid::now_v7()).unwrap();
        meta.add("correlation_id", Uuid::now_v7()).unwrap();
        meta.add("causation_id", Uuid::now_v7()).unwrap();
        meta.add("flatten_val", Uuid::now_v7()).unwrap();

        #[cfg(feature = "context")]
        bus::event(&mut ExampleBusContext::new(&txn, meta), event.clone())
            .await
            .unwrap();

        #[cfg(not(feature = "context"))]
        bus::event(&mut txn, event.clone(), &meta).await.unwrap();

        let row = sqlx::query(SELECT_JOB_SQL)
            .bind(HandlerScheduled::HANDLER_IDENTITY)
            .bind(StateTestEvent::EVENT_IDENTITY)
            .fetch_one(&mut *txn)
            .await
            .unwrap();

        assert_eq!(row.get::<String, _>("queue"), "default".to_string());
        assert_eq!(row.get::<i32, _>("priority"), 21);
        assert_eq!(row.get::<i32, _>("max_attempts"), 100);
        assert_eq!(row.get::<i32, _>("execution_timeout_sec"), 333);

        let row_meta: serde_json::Value =
            row.get::<sqlx::types::Json<serde_json::Value>, _>("meta").0;
        assert_eq!(row_meta, serde_json::to_value(&meta).unwrap());

        let row_tags: serde_json::Value =
            row.get::<sqlx::types::Json<serde_json::Value>, _>("tags").0;
        assert_eq!(row_tags, serde_json::json!(["11", "22", "33"]));

        txn.rollback().await.unwrap();
    }

    #[derive(Default)]
    pub struct HandlerExecuting;
    #[BusEventHandlerDatabase]
    impl IEventHandlerDatabase<StateTestEvent> for HandlerExecuting {
        const QUEUE: &'static str = "default";
        const PRIORITY: u32 = 10;
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

        #[cfg(feature = "sqlx-postgres")]
        async fn handle(
            &self,
            _db: &sqlx::PgPool,
            _event: &StateTestEvent,
            _metadata: &crate::contracts::meta::BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }

        #[cfg(feature = "sqlx-mysql")]
        async fn handle(
            &self,
            _db: &sqlx::MySqlPool,
            _event: &StateTestEvent,
            _metadata: &crate::contracts::meta::BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_unique_executing_replace() {
        setup_bus().await;
        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let mut txn = pool.begin().await.unwrap();

        sqlx::query("DELETE FROM bus_jobs")
            .execute(&mut *txn)
            .await
            .unwrap();

        let mut hasher = XxHash3_64::with_seed(0);
        hasher.write(HandlerExecuting::HANDLER_IDENTITY.as_bytes());
        hasher.write(StateTestEvent::EVENT_IDENTITY.as_bytes());
        let hash_type_name = hasher.finish() as i64;

        let event = StateTestEvent {
            code: "UNIQUE_101".to_string(),
        };

        insert_job(
            &mut txn,
            "executing",
            hash_type_name,
            "default",
            StateTestEvent::EVENT_IDENTITY,
            HandlerExecuting::HANDLER_IDENTITY,
            serde_json::to_value(&event).unwrap(),
            serde_json::to_value(&BusMetadata::default()).unwrap(),
        )
        .await;

        let mut meta = BusMetadata::default();
        meta.add("attempt", 1).unwrap();
        meta.add("request_id", Uuid::now_v7()).unwrap();
        meta.add("correlation_id", Uuid::now_v7()).unwrap();
        meta.add("causation_id", Uuid::now_v7()).unwrap();
        meta.add("flatten_val", Uuid::now_v7()).unwrap();

        #[cfg(feature = "context")]
        bus::event(&mut ExampleBusContext::new(&txn, meta), event.clone())
            .await
            .unwrap();

        #[cfg(not(feature = "context"))]
        bus::event(&mut txn, event.clone(), &meta).await.unwrap();

        let row = sqlx::query(SELECT_JOB_SQL)
            .bind(HandlerExecuting::HANDLER_IDENTITY)
            .bind(StateTestEvent::EVENT_IDENTITY)
            .fetch_one(&mut *txn)
            .await
            .unwrap();

        assert_eq!(row.get::<i32, _>("max_attempts"), 100);

        let row_meta: serde_json::Value =
            row.get::<sqlx::types::Json<serde_json::Value>, _>("meta").0;
        assert_eq!(row_meta, serde_json::to_value(&meta).unwrap());

        let row_tags: serde_json::Value =
            row.get::<sqlx::types::Json<serde_json::Value>, _>("tags").0;
        assert_eq!(row_tags, serde_json::json!(["11", "22", "33"]));

        txn.rollback().await.unwrap();
    }

    #[derive(Default)]
    pub struct HandlerRetryable;
    #[BusEventHandlerDatabase]
    impl IEventHandlerDatabase<StateTestEvent> for HandlerRetryable {
        const QUEUE: &'static str = "default";
        const PRIORITY: u32 = 21;
        const MAX_ATTEMPTS: Option<u32> = Some(10);
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

        #[cfg(feature = "sqlx-postgres")]
        async fn handle(
            &self,
            _db: &sqlx::PgPool,
            _event: &StateTestEvent,
            _metadata: &crate::contracts::meta::BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }

        #[cfg(feature = "sqlx-mysql")]
        async fn handle(
            &self,
            _db: &sqlx::MySqlPool,
            _event: &StateTestEvent,
            _metadata: &crate::contracts::meta::BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_unique_retryable_replace() {
        setup_bus().await;
        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let mut txn = pool.begin().await.unwrap();

        sqlx::query("DELETE FROM bus_jobs")
            .execute(&mut *txn)
            .await
            .unwrap();

        let mut hasher = XxHash3_64::with_seed(0);
        hasher.write(HandlerRetryable::HANDLER_IDENTITY.as_bytes());
        hasher.write(StateTestEvent::EVENT_IDENTITY.as_bytes());
        let hash_type_name = hasher.finish() as i64;

        insert_job(
            &mut txn,
            "retryable",
            hash_type_name,
            "NOT_DEFAULT",
            StateTestEvent::EVENT_IDENTITY,
            HandlerRetryable::HANDLER_IDENTITY,
            serde_json::to_value(&StateTestEvent {
                code: "UNIQUE_101".to_string(),
            })
            .unwrap(),
            serde_json::to_value(&BusMetadata::default()).unwrap(),
        )
        .await;

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
        bus::event(&mut ExampleBusContext::new(&txn, meta), event.clone())
            .await
            .unwrap();

        #[cfg(not(feature = "context"))]
        bus::event(&mut txn, event.clone(), &meta).await.unwrap();

        let row = sqlx::query(SELECT_JOB_SQL)
            .bind(HandlerRetryable::HANDLER_IDENTITY)
            .bind(StateTestEvent::EVENT_IDENTITY)
            .fetch_one(&mut *txn)
            .await
            .unwrap();

        assert_eq!(row.get::<i32, _>("execution_timeout_sec"), 333);

        let row_event: serde_json::Value = row
            .get::<sqlx::types::Json<serde_json::Value>, _>("payload")
            .0;
        assert_eq!(row_event, serde_json::to_value(&event).unwrap());

        let row_meta: serde_json::Value =
            row.get::<sqlx::types::Json<serde_json::Value>, _>("meta").0;
        assert_eq!(row_meta, serde_json::to_value(&meta).unwrap());

        let row_tags: serde_json::Value =
            row.get::<sqlx::types::Json<serde_json::Value>, _>("tags").0;
        assert_eq!(row_tags, serde_json::json!(["11", "22", "33"]));

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

        #[cfg(feature = "sqlx-postgres")]
        async fn handle(
            &self,
            _db: &sqlx::PgPool,
            _event: &StateTestEvent,
            _metadata: &crate::contracts::meta::BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }

        #[cfg(feature = "sqlx-mysql")]
        async fn handle(
            &self,
            _db: &sqlx::MySqlPool,
            _event: &StateTestEvent,
            _metadata: &crate::contracts::meta::BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_unique_completed_replace() {
        setup_bus().await;
        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let mut txn = pool.begin().await.unwrap();

        sqlx::query("DELETE FROM bus_jobs")
            .execute(&mut *txn)
            .await
            .unwrap();

        let mut hasher = XxHash3_64::with_seed(0);
        hasher.write(HandlerCompleted::HANDLER_IDENTITY.as_bytes());
        hasher.write(StateTestEvent::EVENT_IDENTITY.as_bytes());
        let hash_type_name = hasher.finish() as i64;

        let mut insert_meta = BusMetadata::default();
        insert_meta.add("custom_key", 1).unwrap();

        insert_job(
            &mut txn,
            "completed",
            hash_type_name,
            "NOT_DEFAULT",
            StateTestEvent::EVENT_IDENTITY,
            HandlerCompleted::HANDLER_IDENTITY,
            serde_json::to_value(&StateTestEvent {
                code: "UNIQUE_101".to_string(),
            })
            .unwrap(),
            serde_json::to_value(&BusMetadata::default()).unwrap(),
        )
        .await;

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
        bus::event(&mut ExampleBusContext::new(&txn, meta), event.clone())
            .await
            .unwrap();

        #[cfg(not(feature = "context"))]
        bus::event(&mut txn, event.clone(), &meta).await.unwrap();

        let row = sqlx::query(SELECT_JOB_SQL)
            .bind(HandlerCompleted::HANDLER_IDENTITY)
            .bind(StateTestEvent::EVENT_IDENTITY)
            .fetch_one(&mut *txn)
            .await
            .unwrap();

        let row_meta: serde_json::Value =
            row.get::<sqlx::types::Json<serde_json::Value>, _>("meta").0;
        assert_eq!(row_meta, serde_json::to_value(&meta).unwrap());

        let row_tags: serde_json::Value =
            row.get::<sqlx::types::Json<serde_json::Value>, _>("tags").0;
        assert_eq!(row_tags, serde_json::json!(["11", "22", "33"]));

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

        #[cfg(feature = "sqlx-postgres")]
        async fn handle(
            &self,
            _db: &sqlx::PgPool,
            _event: &StateTestEvent,
            _metadata: &crate::contracts::meta::BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }

        #[cfg(feature = "sqlx-mysql")]
        async fn handle(
            &self,
            _db: &sqlx::MySqlPool,
            _event: &StateTestEvent,
            _metadata: &crate::contracts::meta::BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_unique_cancelled_replace() {
        setup_bus().await;
        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let mut txn = pool.begin().await.unwrap();

        sqlx::query("DELETE FROM bus_jobs")
            .execute(&mut *txn)
            .await
            .unwrap();

        let mut hasher = XxHash3_64::with_seed(0);
        hasher.write(HandlerCancelled::HANDLER_IDENTITY.as_bytes());
        hasher.write(StateTestEvent::EVENT_IDENTITY.as_bytes());
        let hash_type_name = hasher.finish() as i64;

        let mut insert_meta = BusMetadata::default();
        insert_meta.add("custom_key", 1).unwrap();

        insert_job(
            &mut txn,
            "cancelled",
            hash_type_name,
            "NOT_DEFAULT",
            StateTestEvent::EVENT_IDENTITY,
            HandlerCancelled::HANDLER_IDENTITY,
            serde_json::to_value(&StateTestEvent {
                code: "UNIQUE_101".to_string(),
            })
            .unwrap(),
            serde_json::to_value(&BusMetadata::default()).unwrap(),
        )
        .await;

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
        bus::event(&mut ExampleBusContext::new(&txn, meta), event.clone())
            .await
            .unwrap();

        #[cfg(not(feature = "context"))]
        bus::event(&mut txn, event.clone(), &meta).await.unwrap();

        let row = sqlx::query(SELECT_JOB_SQL)
            .bind(HandlerCancelled::HANDLER_IDENTITY)
            .bind(StateTestEvent::EVENT_IDENTITY)
            .fetch_one(&mut *txn)
            .await
            .unwrap();

        let row_meta: serde_json::Value =
            row.get::<sqlx::types::Json<serde_json::Value>, _>("meta").0;
        assert_eq!(row_meta, serde_json::to_value(&meta).unwrap());

        let row_tags: serde_json::Value =
            row.get::<sqlx::types::Json<serde_json::Value>, _>("tags").0;
        assert_eq!(row_tags, serde_json::json!(["11", "22", "33"]));

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

        #[cfg(feature = "sqlx-postgres")]
        async fn handle(
            &self,
            _db: &sqlx::PgPool,
            _event: &StateTestEvent,
            _metadata: &crate::contracts::meta::BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }

        #[cfg(feature = "sqlx-mysql")]
        async fn handle(
            &self,
            _db: &sqlx::MySqlPool,
            _event: &StateTestEvent,
            _metadata: &crate::contracts::meta::BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_unique_discarded_replace() {
        setup_bus().await;
        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let mut txn = pool.begin().await.unwrap();

        sqlx::query("DELETE FROM bus_jobs")
            .execute(&mut *txn)
            .await
            .unwrap();

        let mut hasher = XxHash3_64::with_seed(0);
        hasher.write(HandlerDiscarded::HANDLER_IDENTITY.as_bytes());
        hasher.write(StateTestEvent::EVENT_IDENTITY.as_bytes());
        let hash_type_name = hasher.finish() as i64;

        let mut insert_meta = BusMetadata::default();
        insert_meta.add("custom_key", 1).unwrap();

        insert_job(
            &mut txn,
            "discarded",
            hash_type_name,
            "NOT_DEFAULT",
            StateTestEvent::EVENT_IDENTITY,
            HandlerDiscarded::HANDLER_IDENTITY,
            serde_json::to_value(&StateTestEvent {
                code: "UNIQUE_101".to_string(),
            })
            .unwrap(),
            serde_json::to_value(&BusMetadata::default()).unwrap(),
        )
        .await;

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
        bus::event(&mut ExampleBusContext::new(&txn, meta), event.clone())
            .await
            .unwrap();

        #[cfg(not(feature = "context"))]
        bus::event(&mut txn, event.clone(), &meta).await.unwrap();

        let row = sqlx::query(SELECT_JOB_SQL)
            .bind(HandlerDiscarded::HANDLER_IDENTITY)
            .bind(StateTestEvent::EVENT_IDENTITY)
            .fetch_one(&mut *txn)
            .await
            .unwrap();

        let row_meta: serde_json::Value =
            row.get::<sqlx::types::Json<serde_json::Value>, _>("meta").0;
        assert_eq!(row_meta, serde_json::to_value(&meta).unwrap());

        let row_tags: serde_json::Value =
            row.get::<sqlx::types::Json<serde_json::Value>, _>("tags").0;
        assert_eq!(row_tags, serde_json::json!(["11", "22", "33"]));

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

        #[cfg(feature = "sqlx-postgres")]
        async fn handle(
            &self,
            _db: &sqlx::PgPool,
            _event: &StateTestEvent,
            _metadata: &crate::contracts::meta::BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }

        #[cfg(feature = "sqlx-mysql")]
        async fn handle(
            &self,
            _db: &sqlx::MySqlPool,
            _event: &StateTestEvent,
            _metadata: &crate::contracts::meta::BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_unique_no_replace() {
        setup_bus().await;
        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let mut txn = pool.begin().await.unwrap();

        sqlx::query("DELETE FROM bus_jobs")
            .execute(&mut *txn)
            .await
            .unwrap();

        let mut hasher = XxHash3_64::with_seed(0);
        hasher.write(HandlerNoReplace::HANDLER_IDENTITY.as_bytes());
        hasher.write(StateTestEvent::EVENT_IDENTITY.as_bytes());
        let hash_type_name = hasher.finish() as i64;

        insert_job(
            &mut txn,
            "available",
            hash_type_name,
            "NOT_DEFAULT",
            StateTestEvent::EVENT_IDENTITY,
            HandlerNoReplace::HANDLER_IDENTITY,
            serde_json::to_value(&StateTestEvent {
                code: "UNIQUE_101".to_string(),
            })
            .unwrap(),
            serde_json::to_value(&BusMetadata::default()).unwrap(),
        )
        .await;

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
        bus::event(&mut ExampleBusContext::new(&txn, meta), event.clone())
            .await
            .unwrap();

        #[cfg(not(feature = "context"))]
        bus::event(&mut txn, event.clone(), &meta).await.unwrap();

        let row = sqlx::query(SELECT_JOB_SQL)
            .bind(HandlerNoReplace::HANDLER_IDENTITY)
            .bind(StateTestEvent::EVENT_IDENTITY)
            .fetch_one(&mut *txn)
            .await
            .unwrap();

        assert_eq!(row.get::<String, _>("queue"), "NOT_DEFAULT".to_string());
        assert_eq!(row.get::<i32, _>("priority"), 10);
        assert_eq!(row.get::<i32, _>("max_attempts"), 10);
        assert_eq!(row.get::<i32, _>("execution_timeout_sec"), 100);

        let row_meta: serde_json::Value =
            row.get::<sqlx::types::Json<serde_json::Value>, _>("meta").0;
        assert_eq!(
            row_meta,
            serde_json::to_value(BusMetadata::default()).unwrap()
        );

        let row_tags: serde_json::Value =
            row.get::<sqlx::types::Json<serde_json::Value>, _>("tags").0;
        assert_eq!(row_tags, serde_json::json!(["1", "2", "3"]));

        let sql = "SELECT COUNT(*) FROM bus_jobs WHERE type_name_handler = ";

        #[cfg(feature = "sqlx-postgres")]
        let mut qb = sqlx::QueryBuilder::<sqlx::Postgres>::new(sql);

        #[cfg(feature = "sqlx-mysql")]
        let mut qb = sqlx::QueryBuilder::<sqlx::MySql>::new(sql);

        qb.push_bind(HandlerNoReplace::HANDLER_IDENTITY);

        let count: i64 = qb
            .build_query_as::<(i64,)>()
            .fetch_one(&mut *txn)
            .await
            .unwrap()
            .0;

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

        #[cfg(feature = "sqlx-postgres")]
        async fn handle(
            &self,
            _db: &sqlx::PgPool,
            _event: &StateTestEvent,
            _metadata: &crate::contracts::meta::BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }

        #[cfg(feature = "sqlx-mysql")]
        async fn handle(
            &self,
            _db: &sqlx::MySqlPool,
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
        let mut txn = pool.begin().await.unwrap();

        sqlx::query("DELETE FROM bus_jobs")
            .execute(&mut *txn)
            .await
            .unwrap();

        let event = StateTestEvent {
            code: "TIME_808".to_string(),
        };

        #[cfg(feature = "context")]
        let ctx = &mut ExampleBusContext::new(&txn, meta);
        #[cfg(feature = "context")]
        bus::event(&mut ctx, event.clone()).await.unwrap();

        #[cfg(not(feature = "context"))]
        bus::event(&mut txn, event.clone(), &BusMetadata::default())
            .await
            .unwrap();

        #[cfg(feature = "sqlx-mysql")]
        let sql =
            "UPDATE bus_jobs SET inserted_at = now() - INTERVAL 1 HOUR WHERE type_name_handler = ?";
        #[cfg(feature = "sqlx-postgres")]
        let sql = "UPDATE bus_jobs SET inserted_at = now() - interval '1 hour' WHERE type_name_handler = $1";

        sqlx::query(sql)
            .bind(TimeWindowHandler::HANDLER_IDENTITY)
            .execute(&mut *txn)
            .await
            .unwrap();

        #[cfg(feature = "context")]
        bus::event(&mut ctx, event.clone()).await.unwrap();

        #[cfg(not(feature = "context"))]
        bus::event(&mut txn, event.clone(), &BusMetadata::default())
            .await
            .unwrap();

        #[cfg(feature = "sqlx-mysql")]
        let sql = "SELECT COUNT(*) FROM bus_jobs WHERE type_name_handler = ?";
        #[cfg(feature = "sqlx-postgres")]
        let sql = "SELECT COUNT(*) FROM bus_jobs WHERE type_name_handler = $1";

        let count: i64 = sqlx::query_scalar(sql)
            .bind(TimeWindowHandler::HANDLER_IDENTITY)
            .fetch_one(&mut *txn)
            .await
            .unwrap();

        assert_eq!(
            count, 2,
            "Should create a new row because the old one is outside the 10s window"
        );

        #[cfg(feature = "context")]
        bus::event(&mut ctx, event.clone()).await.unwrap();

        #[cfg(not(feature = "context"))]
        bus::event(&mut txn, event.clone(), &BusMetadata::default())
            .await
            .unwrap();

        #[cfg(feature = "sqlx-mysql")]
        let sql = "SELECT COUNT(*) FROM bus_jobs WHERE type_name_handler = ?";
        #[cfg(feature = "sqlx-postgres")]
        let sql = "SELECT COUNT(*) FROM bus_jobs WHERE type_name_handler = $1";

        let count_final: i64 = sqlx::query_scalar(sql)
            .bind(TimeWindowHandler::HANDLER_IDENTITY)
            .fetch_one(&mut *txn)
            .await
            .unwrap();

        assert_eq!(
            count_final, 2,
            "Should NOT create a third row because the second one is still fresh"
        );

        txn.rollback().await.unwrap();
    }
}
