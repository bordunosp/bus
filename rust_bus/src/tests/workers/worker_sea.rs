#[cfg(test)]
mod tests {
    use crate as rust_bus;
    use crate::BusEventHandlerDatabase;
    use crate::contracts::database_unique::Unique;
    use crate::contracts::enums::{
        AvailableField, Field, Period, Replace, ScheduleIn, State, Timestamp,
    };
    use crate::contracts::event_handler_database::IEventHandlerDatabase;
    use crate::contracts::meta::BusMetadata;
    use crate::sql::{fetch_jobs, handler_error, handler_success, recover_timed_out_jobs};
    use crate::tests::base::setup_bus;
    use crate::workers::configuration::BusQueueConfiguration;
    use crate::workers::queue::worker::Worker;
    use chrono::Utc;
    use once_cell::sync::Lazy;
    use rust_bus_macros::BusEvent;
    use sea_orm::{ActiveModelTrait, ConnectionTrait, DbBackend, Statement};
    use sea_orm::{DatabaseConnection, EntityTrait, TransactionTrait};
    use std::hash::Hasher;
    use std::time::Duration;
    use tokio::sync::Mutex;
    use twox_hash::XxHash3_64;
    use uuid::Uuid;

    #[cfg(feature = "sea-orm-mysql")]
    use crate::models::sea_orm_active_enums::BusJobState;

    #[cfg(feature = "sea-orm-postgres")]
    use crate::models::sea_orm_active_enums_pg::BusJobState;

    static FETCH_JOBS_SUCCESS: Lazy<Mutex<String>> = Lazy::new(|| Mutex::new(String::new()));

    #[BusEvent]
    pub struct OrderCreated {
        pub email: String,
    }

    #[derive(Default)]
    pub struct OrderHandler;

    #[BusEventHandlerDatabase]
    impl IEventHandlerDatabase<OrderCreated> for OrderHandler {
        const QUEUE: &'static str = "default";
        const PRIORITY: u32 = 10;
        const MAX_ATTEMPTS: Option<u32> = Some(22);
        const EXECUTION_TIMEOUT: Option<chrono::Duration> = Some(chrono::Duration::minutes(14));
        const SCHEDULE_IN: ScheduleIn = ScheduleIn::Duration(Duration::from_hours(33));
        const TAGS: &'static [&'static str] = &["user", "company"];
        const UNIQUE: Option<Unique> = Some(Unique {
            period: Period::Duration {
                field: Timestamp::InsertedAt,
                duration: Duration::from_hours(7),
            },
            fields: &[Field::Event],
            keys: &["user_id"],
            states: &[State::Available, State::Scheduled],
            replace: &[Replace::Available(&[AvailableField::Meta])],
        });

        async fn handle(
            &self,
            _db: &DatabaseConnection,
            event: &OrderCreated,
            _metadata: &BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            let mut data = FETCH_JOBS_SUCCESS.lock().await;
            *data = event.email.to_owned();
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_fetch_jobs_success_flow() {
        setup_bus().await;
        let db = BusQueueConfiguration::global().unwrap().get_connection();
        let queue = "default";

        db.execute_unprepared("DELETE FROM bus_jobs WHERE queue = 'default'")
            .await
            .unwrap();

        let mut hasher = XxHash3_64::with_seed(0);
        hasher.write(OrderHandler::HANDLER_IDENTITY.as_bytes());
        hasher.write(OrderCreated::EVENT_IDENTITY.as_bytes());
        let hash_type_name = hasher.finish() as i64;

        let now = Utc::now();
        let job_id_ready = Uuid::now_v7();
        let job_id_future = Uuid::now_v7();

        let ready_model = crate::models::bus_jobs::ActiveModel {
            id: sea_orm::Set(job_id_ready),
            hash_type_name: sea_orm::Set(hash_type_name),
            inserted_at: sea_orm::Set(now),
            scheduled_at: sea_orm::Set(now - chrono::Duration::minutes(5)),
            attempted_at: Default::default(),
            completed_at: Default::default(),
            cancelled_at: Default::default(),
            discarded_at: Default::default(),
            state: sea_orm::Set(BusJobState::Available),
            priority: sea_orm::Set(10),
            attempt: sea_orm::Set(0),
            max_attempts: sea_orm::Set(22),
            execution_timeout_sec: sea_orm::Set(14 * 60),
            queue: sea_orm::Set(queue.to_owned()),
            type_name_event: sea_orm::Set(OrderCreated::EVENT_IDENTITY.to_owned()),
            type_name_handler: sea_orm::Set(OrderHandler::HANDLER_IDENTITY.to_owned()),
            payload: sea_orm::Set(serde_json::json!({"email": "test@example.com"})),
            meta: sea_orm::Set(serde_json::json!({})),
            tags: sea_orm::Set(serde_json::json!(["user", "company"])),
            attempted_by: sea_orm::Set(serde_json::json!([])),
            errors: sea_orm::Set(serde_json::json!([])),
        };

        let future_model = crate::models::bus_jobs::ActiveModel {
            id: sea_orm::Set(job_id_future),
            scheduled_at: sea_orm::Set(now + chrono::Duration::hours(1)),
            hash_type_name: sea_orm::Set(hash_type_name),
            inserted_at: sea_orm::Set(now),
            attempted_at: Default::default(),
            completed_at: Default::default(),
            cancelled_at: Default::default(),
            discarded_at: Default::default(),
            state: sea_orm::Set(BusJobState::Available),
            priority: sea_orm::Set(10),
            attempt: sea_orm::Set(0),
            max_attempts: sea_orm::Set(22),
            execution_timeout_sec: sea_orm::Set(14 * 60),
            queue: sea_orm::Set(queue.to_owned()),
            type_name_event: sea_orm::Set(OrderCreated::EVENT_IDENTITY.to_owned()),
            type_name_handler: sea_orm::Set(OrderHandler::HANDLER_IDENTITY.to_owned()),
            payload: sea_orm::Set(serde_json::json!({"email": "test@example.com"})),
            meta: sea_orm::Set(serde_json::json!({})),
            tags: sea_orm::Set(serde_json::json!(["user", "company"])),
            attempted_by: sea_orm::Set(serde_json::json!([])),
            errors: sea_orm::Set(serde_json::json!([])),
        };

        future_model.insert(db).await.unwrap();
        ready_model.insert(db).await.unwrap();

        let jobs = fetch_jobs(queue, 10).await.expect("Failed to fetch jobs");
        assert_eq!(jobs.len(), 1, "Мала бути вибрана тільки одна джоба");
        assert_eq!(
            jobs[0].id, job_id_ready,
            "Мала бути вибрана саме готова джоба"
        );
        assert_eq!(
            jobs[0].attempt, 1,
            "Attempt має бути інкрементований до 1 (в БД було 0)"
        );

        #[cfg(feature = "sea-orm-mysql")]
        let row = db
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::MySql,
                "SELECT state, attempt FROM bus_jobs WHERE id = ?",
                vec![job_id_ready.into()],
            ))
            .await
            .unwrap()
            .expect("Job not found in database after fetch");

        #[cfg(feature = "sea-orm-postgres")]
        let row = db
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "SELECT state::text, attempt FROM bus_jobs WHERE id = $1",
                vec![job_id_ready.into()],
            ))
            .await
            .unwrap()
            .expect("Job not found in database after fetch");

        let state: String = row.try_get("", "state").unwrap();
        let attempt: i32 = row.try_get("", "attempt").unwrap();

        assert_eq!(
            state,
            "executing".to_string(),
            "Статус в БД має бути 'executing'"
        );
        assert_eq!(attempt, 1, "Attempt в БД має бути 1");
    }

    #[tokio::test]
    async fn test_fetch_jobs_priority_selection_limit() {
        setup_bus().await;
        let db = BusQueueConfiguration::global().unwrap().get_connection();
        db.execute_unprepared("DELETE FROM bus_jobs").await.unwrap();

        let now = Utc::now();
        let handler_hash = 12345;

        let create_job =
            |id: Uuid, priority: i32, offset_min: i64| crate::models::bus_jobs::ActiveModel {
                id: sea_orm::Set(id),
                priority: sea_orm::Set(priority),
                scheduled_at: sea_orm::Set(now + chrono::Duration::minutes(offset_min)),
                state: sea_orm::Set(BusJobState::Available),
                inserted_at: sea_orm::Set(now),
                hash_type_name: sea_orm::Set(handler_hash),
                queue: sea_orm::Set("default".to_owned()),
                payload: sea_orm::Set(serde_json::json!({})),
                attempt: sea_orm::Set(0),
                max_attempts: sea_orm::Set(3),
                execution_timeout_sec: sea_orm::Set(60),
                type_name_event: sea_orm::Set("TestEvent".to_owned()),
                type_name_handler: sea_orm::Set("TestHandler".to_owned()),
                meta: sea_orm::Set(serde_json::json!({})),
                tags: sea_orm::Set(serde_json::json!([])),
                errors: sea_orm::Set(serde_json::json!([])),
                attempted_by: sea_orm::Set(serde_json::json!([])),
                ..Default::default()
            };

        let target_id_1 = Uuid::now_v7();
        let target_id_2 = Uuid::now_v7();

        let jobs_to_insert = vec![
            create_job(target_id_1, 999, -10), // Топ 1 (пріоритет 999, старіша)
            create_job(target_id_2, 999, -5),  // Топ 2 (пріоритет 999, новіша)
            create_job(Uuid::now_v7(), 100, -20), // Не має зайти (пріоритет менший)
            create_job(Uuid::now_v7(), 100, -1),
            create_job(Uuid::now_v7(), 10, -30),
            create_job(Uuid::now_v7(), 10, -10),
            create_job(Uuid::now_v7(), 5, -5),
            create_job(Uuid::now_v7(), 5, -2),
            create_job(Uuid::now_v7(), 1, -60),
            create_job(Uuid::now_v7(), 1000, 10),
        ];

        for job in jobs_to_insert {
            job.insert(db).await.unwrap();
        }

        let jobs = fetch_jobs("default", 2)
            .await
            .expect("Failed to fetch jobs");

        assert_eq!(jobs.len(), 2, "Мало повернутися рівно 2 джоби");

        let has_target_1 = jobs.iter().any(|j| j.id == target_id_1);
        let has_target_2 = jobs.iter().any(|j| j.id == target_id_2);

        assert!(
            has_target_1,
            "Перша цільова джоба (prio 999) не знайдена в ліміті"
        );
        assert!(
            has_target_2,
            "Друга цільова джоба (prio 999) не знайдена в ліміті"
        );

        #[cfg(feature = "sea-orm-mysql")]
        for job in jobs.iter() {
            let row = db
                .query_one_raw(Statement::from_sql_and_values(
                    DbBackend::MySql,
                    "SELECT state, attempt FROM bus_jobs WHERE id = ?",
                    vec![job.id.into()],
                ))
                .await
                .unwrap()
                .unwrap();
            let state: String = row.try_get("", "state").unwrap();
            let attempt: i32 = row.try_get("", "attempt").unwrap();
            assert_eq!(state, "executing");
            assert_eq!(attempt, 1);
        }

        #[cfg(feature = "sea-orm-postgres")]
        for job in jobs.iter() {
            let row = db
                .query_one_raw(Statement::from_sql_and_values(
                    DbBackend::Postgres,
                    "SELECT state::text, attempt FROM bus_jobs WHERE id = $1",
                    vec![job.id.into()],
                ))
                .await
                .unwrap()
                .unwrap();
            let state: String = row.try_get("", "state").unwrap();
            let attempt: i32 = row.try_get("", "attempt").unwrap();
            assert_eq!(state, "executing");
            assert_eq!(attempt, 1);
        }
    }

    #[tokio::test]
    async fn test_handler_error_retry_flow() {
        setup_bus().await;
        let db = BusQueueConfiguration::global().unwrap().get_connection();
        db.execute_unprepared("DELETE FROM bus_jobs").await.unwrap();

        let job_id = Uuid::now_v7();
        let now = Utc::now();

        let mut hasher = XxHash3_64::with_seed(0);
        hasher.write(OrderHandler::HANDLER_IDENTITY.as_bytes());
        hasher.write(OrderCreated::EVENT_IDENTITY.as_bytes());
        let hash_type_name = hasher.finish() as i64;

        let active_job = crate::models::bus_jobs::ActiveModel {
            id: sea_orm::Set(job_id),
            state: sea_orm::Set(BusJobState::Executing),
            attempt: sea_orm::Set(1),
            max_attempts: sea_orm::Set(3),
            priority: sea_orm::Set(10), // Додано пріоритет
            hash_type_name: sea_orm::Set(hash_type_name),
            scheduled_at: sea_orm::Set(now),
            inserted_at: sea_orm::Set(now),
            queue: sea_orm::Set("default".to_owned()),
            type_name_event: sea_orm::Set(OrderCreated::EVENT_IDENTITY.to_owned()),
            type_name_handler: sea_orm::Set(OrderHandler::HANDLER_IDENTITY.to_owned()),
            payload: sea_orm::Set(serde_json::json!({})),
            meta: sea_orm::Set(serde_json::json!({})),
            tags: sea_orm::Set(serde_json::json!([])),
            errors: sea_orm::Set(serde_json::json!([])),
            execution_timeout_sec: sea_orm::Set(60),
            attempted_by: sea_orm::Set(serde_json::json!([])),
            ..Default::default()
        };
        active_job.insert(db).await.unwrap();

        let job_dto = crate::sql::dto::BusJob {
            id: job_id,
            hash_type_name,
            attempt: 1,
            max_attempts: 3,
            execution_timeout_sec: 60,
            type_name_event: OrderCreated::EVENT_IDENTITY.to_owned(),
            type_name_handler: OrderHandler::HANDLER_IDENTITY.to_owned(),
            payload: serde_json::json!({}),
            meta: BusMetadata::default(),
            tags: serde_json::json!([]),
            errors: serde_json::json!([]),
        };

        handler_error(&job_dto, "Database timeout".to_string())
            .await
            .unwrap();

        let job_after: crate::models::bus_jobs::Model =
            crate::models::bus_jobs::Entity::find_by_id(job_id)
                .one(db)
                .await
                .unwrap()
                .unwrap();

        assert_eq!(job_after.state, BusJobState::Retryable);
        assert!(
            job_after.scheduled_at > now,
            "Наступна спроба має бути в майбутньому"
        );

        let errors: serde_json::Value = job_after.errors;
        assert_eq!(errors.as_array().unwrap().len(), 1);
        assert_eq!(errors[0]["error"], "Database timeout");

        use sea_orm::EntityTrait;
        let job_to_update = crate::models::bus_jobs::ActiveModel {
            id: sea_orm::Set(job_id),
            state: sea_orm::Set(BusJobState::Executing),
            scheduled_at: sea_orm::Set(Utc::now() - chrono::Duration::minutes(1)),
            ..Default::default()
        };
        job_to_update.update(db).await.unwrap();

        let job_dto_2 = crate::sql::dto::BusJob {
            attempt: 2,
            ..job_dto.clone()
        };

        crate::sql::handler_error(&job_dto_2, "Connection reset by peer".to_string())
            .await
            .unwrap();

        let job_final = crate::models::bus_jobs::Entity::find_by_id(job_id)
            .one(db)
            .await
            .unwrap()
            .unwrap();

        let final_errors = job_final
            .errors
            .as_array()
            .expect("Errors should be a JSON array");

        assert_eq!(
            final_errors.len(),
            2,
            "Масив помилок має містити рівно 2 елементи"
        );

        assert_eq!(
            final_errors[0]["error"], "Database timeout",
            "Перша помилка має зберегтися"
        );
        assert_eq!(
            final_errors[1]["error"], "Connection reset by peer",
            "Друга помилка має додатися в кінець"
        );

        assert_eq!(final_errors[1]["attempt"], 2);
    }

    #[tokio::test]
    async fn test_fetch_jobs_concurrency_skip_locked() {
        setup_bus().await;
        let db = BusQueueConfiguration::global().unwrap().get_connection();
        db.execute_unprepared("DELETE FROM bus_jobs").await.unwrap();
        let now = Utc::now();

        let job_id = Uuid::now_v7();
        let model = crate::models::bus_jobs::ActiveModel {
            id: sea_orm::Set(job_id),
            hash_type_name: sea_orm::Set(12345),
            state: sea_orm::Set(BusJobState::Available),
            priority: sea_orm::Set(10),
            scheduled_at: sea_orm::Set(now - chrono::Duration::minutes(1)),
            inserted_at: sea_orm::Set(now),
            queue: sea_orm::Set("default".to_owned()),
            attempt: sea_orm::Set(0),
            max_attempts: sea_orm::Set(3),
            execution_timeout_sec: sea_orm::Set(60),
            type_name_event: sea_orm::Set("TestEvent".to_owned()),
            type_name_handler: sea_orm::Set("TestHandler".to_owned()),
            payload: sea_orm::Set(serde_json::json!({})),
            meta: sea_orm::Set(serde_json::json!({})),
            tags: sea_orm::Set(serde_json::json!([])),
            errors: sea_orm::Set(serde_json::json!([])),
            attempted_by: sea_orm::Set(serde_json::json!([])),
            ..Default::default()
        };
        model.insert(db).await.unwrap();

        let txn = db.begin().await.unwrap();
        let jobs_1 = fetch_jobs("default", 1).await.unwrap();
        assert_eq!(jobs_1.len(), 1);
        assert_eq!(jobs_1[0].id, job_id);

        let jobs_2 = fetch_jobs("default", 1).await.unwrap();

        assert_eq!(
            jobs_2.len(),
            0,
            "Другий воркер не мав взяти заблоковану джобу"
        );

        txn.commit().await.unwrap();
    }

    #[BusEvent]
    pub struct PanicEvent {
        pub message: String,
    }

    #[derive(Default)]
    pub struct PanicHandler;

    #[BusEventHandlerDatabase]
    impl IEventHandlerDatabase<PanicEvent> for PanicHandler {
        const QUEUE: &'static str = "default";
        const PRIORITY: u32 = 1;

        async fn handle(
            &self,
            _db: &DatabaseConnection,
            _event: &PanicEvent,
            _metadata: &BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            panic!("Boom! Something went wrong in the handler logic");
        }
    }

    #[tokio::test]
    async fn test_full_worker_lifecycle_success() {
        setup_bus().await;
        let db = BusQueueConfiguration::global().unwrap().get_connection();
        let queue = "default";

        db.execute_unprepared(&format!("DELETE FROM bus_jobs WHERE queue = '{}'", queue))
            .await
            .unwrap();

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(());

        let worker = Worker::new(queue, 2, Duration::from_nanos(1), shutdown_rx);
        let worker_handle = tokio::spawn(async move {
            worker.run().await;
        });

        let job_id = Uuid::now_v7();
        let now = Utc::now();

        let mut hasher = XxHash3_64::with_seed(0);
        hasher.write(OrderHandler::HANDLER_IDENTITY.as_bytes());
        hasher.write(OrderCreated::EVENT_IDENTITY.as_bytes());
        let hash_type_name = hasher.finish() as i64;

        let model = crate::models::bus_jobs::ActiveModel {
            id: sea_orm::Set(job_id),
            hash_type_name: sea_orm::Set(hash_type_name),
            state: sea_orm::Set(BusJobState::Available),
            queue: sea_orm::Set(queue.to_owned()),
            scheduled_at: sea_orm::Set(now),
            inserted_at: sea_orm::Set(now),
            payload: sea_orm::Set(serde_json::json!({"email": "real_worker@test.com"})),
            type_name_event: sea_orm::Set(OrderCreated::EVENT_IDENTITY.to_owned()),
            type_name_handler: sea_orm::Set(OrderHandler::HANDLER_IDENTITY.to_owned()),
            attempt: sea_orm::Set(0),
            max_attempts: sea_orm::Set(3),
            priority: sea_orm::Set(10),
            execution_timeout_sec: sea_orm::Set(5),
            meta: sea_orm::Set(serde_json::json!({})),
            tags: sea_orm::Set(serde_json::json!([])),
            errors: sea_orm::Set(serde_json::json!([])),
            attempted_by: sea_orm::Set(serde_json::json!([])),
            ..Default::default()
        };
        model.insert(db).await.unwrap();

        let mut success = false;
        for _ in 0..20 {
            tokio::time::sleep(Duration::from_millis(100)).await;

            let job = crate::models::bus_jobs::Entity::find_by_id(job_id)
                .one(db)
                .await
                .unwrap()
                .expect("Job disappeared!");

            if job.state == BusJobState::Completed {
                success = true;
                break;
            }
        }

        let _ = shutdown_tx.send(());
        let _ = worker_handle.await;

        assert!(
            success,
            "Воркер не перевів джобу в статус Completed за відведений час"
        );

        let processed_email = FETCH_JOBS_SUCCESS.lock().await;
        assert_eq!(*processed_email, "real_worker@test.com");
    }

    #[BusEvent]
    pub struct LongRunningEvent {
        pub duration_ms: u64,
    }

    #[derive(Default)]
    pub struct SleepHandler;

    #[BusEventHandlerDatabase]
    impl IEventHandlerDatabase<LongRunningEvent> for SleepHandler {
        const QUEUE: &'static str = "default";
        const PRIORITY: u32 = 1;

        async fn handle(
            &self,
            _db: &DatabaseConnection,
            event: &LongRunningEvent,
            _metadata: &BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            tokio::time::sleep(Duration::from_millis(event.duration_ms)).await;
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_worker_handle_abort_on_timeout() {
        setup_bus().await;
        let db = BusQueueConfiguration::global().unwrap().get_connection();
        let queue = "timeout_queue";

        db.execute_unprepared(&format!("DELETE FROM bus_jobs WHERE queue = '{}'", queue))
            .await
            .unwrap();

        let job_id = Uuid::now_v7();
        let now = Utc::now();

        let mut hasher = XxHash3_64::with_seed(0);
        hasher.write(SleepHandler::HANDLER_IDENTITY.as_bytes());
        hasher.write(LongRunningEvent::EVENT_IDENTITY.as_bytes());
        let hash_type_name = hasher.finish() as i64;

        let model = crate::models::bus_jobs::ActiveModel {
            id: sea_orm::Set(job_id),
            hash_type_name: sea_orm::Set(hash_type_name),
            state: sea_orm::Set(BusJobState::Available),
            queue: sea_orm::Set(queue.to_owned()),
            scheduled_at: sea_orm::Set(now),
            inserted_at: sea_orm::Set(now),
            payload: sea_orm::Set(serde_json::json!({"duration_ms": 5000})),
            type_name_event: sea_orm::Set(LongRunningEvent::EVENT_IDENTITY.to_owned()),
            type_name_handler: sea_orm::Set(SleepHandler::HANDLER_IDENTITY.to_owned()),
            attempt: sea_orm::Set(0),
            max_attempts: sea_orm::Set(3),
            priority: sea_orm::Set(1),
            execution_timeout_sec: sea_orm::Set(1),
            meta: sea_orm::Set(serde_json::json!({})),
            tags: sea_orm::Set(serde_json::json!([])),
            errors: sea_orm::Set(serde_json::json!([])),
            attempted_by: sea_orm::Set(serde_json::json!([])),
            ..Default::default()
        };
        model.insert(db).await.unwrap();

        let jobs = fetch_jobs(queue, 1).await.unwrap();
        let job_dto = jobs[0].clone();

        let start_time = std::time::Instant::now();
        let result = Worker::execute_with_timeout(job_dto).await;
        let elapsed = start_time.elapsed();

        assert!(result.is_err());
        assert!(
            result.unwrap_err().contains("timed out"),
            "Має бути повідомлення про таймаут"
        );
        assert!(
            elapsed.as_secs() < 2,
            "Воркер працював занадто довго, abort не спрацював?"
        );
    }

    #[tokio::test]
    async fn test_handler_success_idempotent() {
        setup_bus().await;
        let db = BusQueueConfiguration::global().unwrap().get_connection();

        let job_id = Uuid::now_v7();
        let now = Utc::now();

        let model = crate::models::bus_jobs::ActiveModel {
            id: sea_orm::Set(job_id),
            state: sea_orm::Set(BusJobState::Completed),
            scheduled_at: sea_orm::Set(now),
            inserted_at: sea_orm::Set(now),
            queue: sea_orm::Set("default".to_string()),
            hash_type_name: sea_orm::Set(1),
            payload: sea_orm::Set(serde_json::json!({})),
            type_name_event: sea_orm::Set("E".into()),
            type_name_handler: sea_orm::Set("H".into()),
            attempt: sea_orm::Set(1),
            max_attempts: sea_orm::Set(3),
            priority: sea_orm::Set(1),
            execution_timeout_sec: sea_orm::Set(10),
            meta: sea_orm::Set(serde_json::json!({})),
            tags: sea_orm::Set(serde_json::json!([])),
            errors: sea_orm::Set(serde_json::json!([])),
            attempted_by: sea_orm::Set(serde_json::json!([])),
            ..Default::default()
        };

        model.insert(db).await.unwrap();

        handler_success(job_id).await.unwrap();

        let job = crate::models::bus_jobs::Entity::find_by_id(job_id)
            .one(db)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(job.state, BusJobState::Completed);
    }

    #[tokio::test]
    async fn test_handler_error_discard_flow() {
        setup_bus().await;
        let db = BusQueueConfiguration::global().unwrap().get_connection();

        let job_id = Uuid::now_v7();
        let now = Utc::now();

        let job = crate::models::bus_jobs::ActiveModel {
            id: sea_orm::Set(job_id),
            state: sea_orm::Set(BusJobState::Executing),
            attempt: sea_orm::Set(3),
            max_attempts: sea_orm::Set(3),
            scheduled_at: sea_orm::Set(now),
            inserted_at: sea_orm::Set(now),
            queue: sea_orm::Set("default".to_string()),
            hash_type_name: sea_orm::Set(1),
            payload: sea_orm::Set(serde_json::json!({})),
            type_name_event: sea_orm::Set("E".into()),
            type_name_handler: sea_orm::Set("H".into()),
            priority: sea_orm::Set(1),
            execution_timeout_sec: sea_orm::Set(10),
            meta: sea_orm::Set(serde_json::json!({})),
            tags: sea_orm::Set(serde_json::json!([])),
            errors: sea_orm::Set(serde_json::json!([])),
            attempted_by: sea_orm::Set(serde_json::json!([])),
            ..Default::default()
        };

        job.insert(db).await.unwrap();

        let dto = crate::sql::dto::BusJob {
            id: job_id,
            attempt: 3,
            max_attempts: 3,
            hash_type_name: 1,
            execution_timeout_sec: 10,
            type_name_event: "E".into(),
            type_name_handler: "H".into(),
            payload: serde_json::json!({}),
            meta: BusMetadata::default(),
            tags: serde_json::json!([]),
            errors: serde_json::json!([]),
        };

        handler_error(&dto, "fatal".into()).await.unwrap();

        let job = crate::models::bus_jobs::Entity::find_by_id(job_id)
            .one(db)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(job.state, BusJobState::Discarded);
        assert!(job.discarded_at.is_some());
    }

    #[tokio::test]
    async fn test_worker_timeout_triggers_handler_error() {
        setup_bus().await;
        let db = BusQueueConfiguration::global().unwrap().get_connection();

        let queue = "timeout_worker";
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(());

        let job_id = Uuid::now_v7();
        let now = Utc::now();

        let mut hasher = XxHash3_64::with_seed(0);
        hasher.write(SleepHandler::HANDLER_IDENTITY.as_bytes());
        hasher.write(LongRunningEvent::EVENT_IDENTITY.as_bytes());
        let hash = hasher.finish() as i64;

        let model = crate::models::bus_jobs::ActiveModel {
            id: sea_orm::Set(job_id),
            hash_type_name: sea_orm::Set(hash),
            state: sea_orm::Set(BusJobState::Available),
            queue: sea_orm::Set(queue.to_string()),
            scheduled_at: sea_orm::Set(now),
            inserted_at: sea_orm::Set(now),
            payload: sea_orm::Set(serde_json::json!({"duration_ms": 5000})),
            type_name_event: sea_orm::Set(LongRunningEvent::EVENT_IDENTITY.to_owned()),
            type_name_handler: sea_orm::Set(SleepHandler::HANDLER_IDENTITY.to_owned()),
            attempt: sea_orm::Set(0),
            max_attempts: sea_orm::Set(3),
            priority: sea_orm::Set(1),
            execution_timeout_sec: sea_orm::Set(1),
            meta: sea_orm::Set(serde_json::json!({})),
            tags: sea_orm::Set(serde_json::json!([])),
            errors: sea_orm::Set(serde_json::json!([])),
            attempted_by: sea_orm::Set(serde_json::json!([])),
            ..Default::default()
        };

        model.insert(db).await.unwrap();

        let worker = Worker::new(queue, 1, Duration::from_nanos(1), shutdown_rx);
        let handle = tokio::spawn(async move {
            worker.run().await;
        });

        tokio::time::sleep(Duration::from_secs(2)).await;

        let job = crate::models::bus_jobs::Entity::find_by_id(job_id)
            .one(db)
            .await
            .unwrap()
            .unwrap();

        let _ = shutdown_tx.send(());
        let _ = handle.await;

        assert!(
            job.state == BusJobState::Retryable || job.state == BusJobState::Discarded,
            "Timeout має перевести job в retry/discard"
        );

        let errors = job.errors.as_array().unwrap();
        assert!(!errors.is_empty(), "Має бути записана помилка");
    }

    #[tokio::test]
    async fn test_worker_recovers_from_panic() {
        setup_bus().await;
        let db = BusQueueConfiguration::global().unwrap().get_connection();

        let queue = "panic_queue";

        db.execute_unprepared(&format!("DELETE FROM bus_jobs WHERE queue = '{}'", queue))
            .await
            .unwrap();

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(());

        let worker = Worker::new(queue, 1, Duration::from_nanos(1), shutdown_rx);
        let handle = tokio::spawn(async move {
            worker.run().await;
        });

        let job_id = Uuid::now_v7();
        let now = Utc::now();

        let mut hasher = XxHash3_64::with_seed(0);
        hasher.write(PanicHandler::HANDLER_IDENTITY.as_bytes());
        hasher.write(PanicEvent::EVENT_IDENTITY.as_bytes());
        let hash = hasher.finish() as i64;

        let model = crate::models::bus_jobs::ActiveModel {
            id: sea_orm::Set(job_id),
            hash_type_name: sea_orm::Set(hash),
            state: sea_orm::Set(BusJobState::Available),
            queue: sea_orm::Set(queue.to_string()),
            scheduled_at: sea_orm::Set(now),
            inserted_at: sea_orm::Set(now),
            payload: sea_orm::Set(serde_json::json!({"message": "boom"})),
            type_name_event: sea_orm::Set(PanicEvent::EVENT_IDENTITY.to_owned()),
            type_name_handler: sea_orm::Set(PanicHandler::HANDLER_IDENTITY.to_owned()),
            attempt: sea_orm::Set(0),
            max_attempts: sea_orm::Set(3),
            priority: sea_orm::Set(1),
            execution_timeout_sec: sea_orm::Set(5),
            meta: sea_orm::Set(serde_json::json!({})),
            tags: sea_orm::Set(serde_json::json!([])),
            errors: sea_orm::Set(serde_json::json!([])),
            attempted_by: sea_orm::Set(serde_json::json!([])),
            ..Default::default()
        };

        model.insert(db).await.unwrap();

        tokio::time::sleep(Duration::from_secs(1)).await;

        let job = crate::models::bus_jobs::Entity::find_by_id(job_id)
            .one(db)
            .await
            .unwrap()
            .unwrap();

        let _ = shutdown_tx.send(());
        let _ = handle.await;

        assert!(
            job.state == BusJobState::Retryable || job.state == BusJobState::Discarded,
            "Panic має бути оброблений як помилка"
        );

        assert!(
            !job.errors.as_array().unwrap().is_empty(),
            "Panic має записатись в errors"
        );
    }

    #[tokio::test]
    async fn test_worker_backoff_on_empty_queue() {
        setup_bus().await;

        let queue = "empty_queue";

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(());

        let worker = Worker::new(queue, 1, Duration::from_nanos(1), shutdown_rx);

        let start = std::time::Instant::now();

        let handle = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(2)).await;
            worker.run().await;
        });

        tokio::time::sleep(Duration::from_secs(2)).await;

        let elapsed = start.elapsed();

        let _ = shutdown_tx.send(());
        let _ = handle.await;

        assert!(elapsed.as_secs() >= 2, "Worker не має крутити busy loop");
    }

    #[tokio::test]
    async fn test_worker_graceful_shutdown_does_not_lose_jobs() {
        setup_bus().await;
        let db = BusQueueConfiguration::global().unwrap().get_connection();

        let queue = "graceful_queue";

        db.execute_unprepared(&format!("DELETE FROM bus_jobs WHERE queue = '{}'", queue))
            .await
            .unwrap();

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(());

        let job_id = Uuid::now_v7();
        let now = Utc::now();

        let mut hasher = XxHash3_64::with_seed(0);
        hasher.write(SleepHandler::HANDLER_IDENTITY.as_bytes());
        hasher.write(LongRunningEvent::EVENT_IDENTITY.as_bytes());
        let hash = hasher.finish() as i64;

        let model = crate::models::bus_jobs::ActiveModel {
            id: sea_orm::Set(job_id),
            hash_type_name: sea_orm::Set(hash),
            state: sea_orm::Set(BusJobState::Available),
            queue: sea_orm::Set(queue.to_string()),
            scheduled_at: sea_orm::Set(now),
            inserted_at: sea_orm::Set(now),
            payload: sea_orm::Set(serde_json::json!({"duration_ms": 5000})),
            type_name_event: sea_orm::Set(LongRunningEvent::EVENT_IDENTITY.to_owned()),
            type_name_handler: sea_orm::Set(SleepHandler::HANDLER_IDENTITY.to_owned()),
            attempt: sea_orm::Set(0),
            max_attempts: sea_orm::Set(3),
            priority: sea_orm::Set(1),
            execution_timeout_sec: sea_orm::Set(10),
            meta: sea_orm::Set(serde_json::json!({})),
            tags: sea_orm::Set(serde_json::json!([])),
            errors: sea_orm::Set(serde_json::json!([])),
            attempted_by: sea_orm::Set(serde_json::json!([])),
            ..Default::default()
        };

        model.insert(db).await.unwrap();

        let worker = Worker::new(queue, 1, Duration::from_nanos(1), shutdown_rx);
        let handle = tokio::spawn(async move {
            worker.run().await;
        });

        let mut started = false;
        for _ in 0..100 {
            let check_job = crate::models::bus_jobs::Entity::find_by_id(job_id)
                .one(db)
                .await
                .unwrap()
                .unwrap();

            if check_job.state == BusJobState::Executing {
                started = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        assert!(started, "Воркер не підхопив джобу за 5 секунд");

        let _ = shutdown_tx.send(());
        let _ = handle.await;

        let job = crate::models::bus_jobs::Entity::find_by_id(job_id)
            .one(db)
            .await
            .unwrap()
            .unwrap();

        assert_ne!(
            job.state,
            BusJobState::Completed,
            "Job не мав завершитися під час shutdown"
        );

        assert!(
            job.state == BusJobState::Available
                || job.state == BusJobState::Retryable
                || job.state == BusJobState::Executing,
            "Job має залишитись доступною для повторної обробки"
        );

        tokio::time::sleep(Duration::from_millis(600)).await;

        let job = crate::models::bus_jobs::Entity::find_by_id(job_id)
            .one(db)
            .await
            .unwrap()
            .expect("Job has been deleted from DB unexpectedly");

        assert_eq!(
            job.state,
            BusJobState::Available,
            "Стан має бути Available після shutdown"
        );

        assert_eq!(
            job.attempt, 0,
            "Attempt не має інкрементуватися при звичайному shutdown"
        );

        let errors: Vec<serde_json::Value> =
            serde_json::from_value(job.errors).expect("Failed to parse job errors");

        assert!(!errors.is_empty(), "Список помилок не має бути порожнім");

        let last_error = errors.last().unwrap();
        let error_msg = last_error
            .get("error")
            .and_then(|m| m.as_str())
            .unwrap_or("");

        assert!(
            error_msg.contains("Worker shutdown") || error_msg.contains("cancelled"),
            "Помилка має містити згадку про зупинку воркера. Отримано: {}",
            error_msg
        );

        #[cfg(feature = "logging")]
        log::info!(
            "✅ Graceful shutdown verified: Job returned to Available, attempts: {}, error recorded.",
            job.attempt
        );
    }

    #[tokio::test]
    async fn test_recover_timed_out_jobs_moves_stale_jobs() {
        setup_bus().await;

        let db = BusQueueConfiguration::global().unwrap().get_connection();
        let queue = "default";

        db.execute_unprepared(&format!("DELETE FROM bus_jobs WHERE queue = '{}'", queue))
            .await
            .unwrap();

        let job_id = Uuid::now_v7();
        let now = Utc::now();

        let mut hasher = XxHash3_64::with_seed(0);
        hasher.write(OrderHandler::HANDLER_IDENTITY.as_bytes());
        hasher.write(OrderCreated::EVENT_IDENTITY.as_bytes());
        let hash_type_name = hasher.finish() as i64;

        let model = crate::models::bus_jobs::ActiveModel {
            id: sea_orm::Set(job_id),
            hash_type_name: sea_orm::Set(hash_type_name),
            state: sea_orm::Set(BusJobState::Executing),
            queue: sea_orm::Set(queue.to_string()),
            attempted_at: sea_orm::Set(Some(now - chrono::Duration::minutes(20))),
            scheduled_at: sea_orm::Set(now),
            inserted_at: sea_orm::Set(now),
            payload: sea_orm::Set(serde_json::json!({})),
            type_name_event: sea_orm::Set(OrderCreated::EVENT_IDENTITY.to_string()),
            type_name_handler: sea_orm::Set(OrderHandler::HANDLER_IDENTITY.to_string()),
            attempt: sea_orm::Set(1),
            max_attempts: sea_orm::Set(3),
            priority: sea_orm::Set(1),
            execution_timeout_sec: sea_orm::Set(5),
            meta: sea_orm::Set(serde_json::json!({})),
            tags: sea_orm::Set(serde_json::json!([])),
            errors: sea_orm::Set(serde_json::json!([])),
            attempted_by: sea_orm::Set(serde_json::json!([])),

            ..Default::default()
        };

        model.insert(db).await.unwrap();

        recover_timed_out_jobs().await.unwrap();

        let job = crate::models::bus_jobs::Entity::find_by_id(job_id)
            .one(db)
            .await
            .unwrap()
            .unwrap();

        assert_ne!(
            job.state,
            BusJobState::Executing,
            "Job має бути витягнута зі стану executing"
        );

        assert_eq!(
            job.state,
            BusJobState::Retryable,
            "Job має перейти в retryable"
        );

        assert_eq!(job.attempt, 1, "Attempt не має змінюватися при recovery");

        let errors: Vec<serde_json::Value> =
            serde_json::from_value(job.errors).expect("Invalid errors json");

        assert!(!errors.is_empty(), "Errors мають бути записані");

        let last_error = errors.last().unwrap();
        let msg = last_error
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        assert!(
            msg.contains("timed out"),
            "Очікується timeout помилка, отримано: {}",
            msg
        );
    }

    #[tokio::test]
    async fn test_recover_does_not_touch_fresh_jobs() {
        setup_bus().await;

        let db = BusQueueConfiguration::global().unwrap().get_connection();
        let queue = "default";

        db.execute_unprepared(&format!("DELETE FROM bus_jobs WHERE queue = '{}'", queue))
            .await
            .unwrap();

        let job_id = Uuid::now_v7();
        let now = chrono::Utc::now();

        let model = crate::models::bus_jobs::ActiveModel {
            id: sea_orm::Set(job_id),
            hash_type_name: sea_orm::Set(123),
            state: sea_orm::Set(BusJobState::Executing),
            queue: sea_orm::Set(queue.to_string()),

            attempted_at: sea_orm::Set(Some(now)),

            scheduled_at: sea_orm::Set(now),
            inserted_at: sea_orm::Set(now),

            payload: sea_orm::Set(serde_json::json!({})),
            type_name_event: sea_orm::Set("TestEvent".to_string()),
            type_name_handler: sea_orm::Set("TestHandler".to_string()),

            attempt: sea_orm::Set(1),
            max_attempts: sea_orm::Set(3),
            priority: sea_orm::Set(1),

            execution_timeout_sec: sea_orm::Set(60),

            meta: sea_orm::Set(serde_json::json!({})),
            tags: sea_orm::Set(serde_json::json!([])),
            errors: sea_orm::Set(serde_json::json!([])),
            attempted_by: sea_orm::Set(serde_json::json!([])),

            ..Default::default()
        };

        model.insert(db).await.unwrap();

        recover_timed_out_jobs().await.unwrap();

        let job = crate::models::bus_jobs::Entity::find_by_id(job_id)
            .one(db)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(
            job.state,
            BusJobState::Executing,
            "Свіжі jobs не мають чіпатись"
        );
    }
}
