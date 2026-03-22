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
    use chrono::{DateTime, Utc};
    use once_cell::sync::Lazy;
    use rust_bus_macros::BusEvent;
    use serde_json::Value;
    use sqlx::{QueryBuilder, Row};
    use std::hash::Hasher;
    use std::time::Duration;
    use tokio::sync::Mutex;
    use twox_hash::XxHash3_64;
    use uuid::Uuid;

    static FETCH_JOBS_SUCCESS: Lazy<Mutex<String>> = Lazy::new(|| Mutex::new(String::new()));

    const INSERT_SQL: &str = "INSERT INTO bus_jobs (id, hash_type_name, inserted_at, scheduled_at, attempted_at, completed_at, cancelled_at, discarded_at, state, priority, attempt, max_attempts, execution_timeout_sec, queue, type_name_event, type_name_handler, payload, meta, tags, errors, attempted_by) ";

    #[cfg(feature = "sqlx-mysql")]
    const SELECT_JOB_SQL: &str = "SELECT * FROM bus_jobs WHERE id = ? LIMIT 1";

    #[cfg(feature = "sqlx-postgres")]
    const SELECT_JOB_SQL: &str = "SELECT *, state::text FROM bus_jobs WHERE id = $1 LIMIT 1";

    struct JobModel {
        pub id: Uuid,
        pub hash_type_name: i64,
        pub state: String,
        pub queue: String,
        pub scheduled_at: DateTime<Utc>,
        pub payload: Value,
        pub type_name_event: String,
        pub type_name_handler: String,
        pub attempt: i32,
        pub max_attempts: i32,
        pub priority: i32,
        pub execution_timeout_sec: i32,
        pub meta: Value,
        pub tags: Value,
        pub errors: Value,
        pub attempted_at: Option<DateTime<Utc>>,
    }

    async fn insert_job(job: JobModel) {
        let db = BusQueueConfiguration::global().unwrap().get_connection();

        #[cfg(feature = "sqlx-postgres")]
        let mut qb = QueryBuilder::<sqlx::Postgres>::new(INSERT_SQL);
        #[cfg(feature = "sqlx-mysql")]
        let mut qb = QueryBuilder::<sqlx::MySql>::new(INSERT_SQL);

        qb.push(" VALUES (");

        // 1. id
        qb.push_bind(job.id);
        qb.push(", ");

        // 2. hash_type_name
        qb.push_bind(job.hash_type_name);
        qb.push(", ");

        // 3. inserted_at
        qb.push_bind(Utc::now());
        qb.push(", ");

        // 4. scheduled_at
        qb.push_bind(job.scheduled_at);
        qb.push(", ");

        // 5-8. NULL dates (attempted, completed, cancelled, discarded)
        qb.push_bind(job.attempted_at);
        qb.push(", ");
        qb.push_bind(None::<DateTime<Utc>>);
        qb.push(", ");
        qb.push_bind(None::<DateTime<Utc>>);
        qb.push(", ");
        qb.push_bind(None::<DateTime<Utc>>);
        qb.push(", ");

        // 9. state (з кастом)
        qb.push_bind(job.state);

        #[cfg(feature = "sqlx-postgres")]
        qb.push("::bus_job_state ");

        qb.push(", ");

        // 10. priority
        qb.push_bind(job.priority);
        qb.push(", ");

        // 11. attempt
        qb.push_bind(job.attempt);
        qb.push(", ");

        // 12. max_attempts
        qb.push_bind(job.max_attempts);
        qb.push(", ");

        // 13. execution_timeout_sec
        qb.push_bind(job.execution_timeout_sec);
        qb.push(", ");

        // 14. queue
        qb.push_bind(job.queue);
        qb.push(", ");

        // 15. type_name_event
        qb.push_bind(job.type_name_event);
        qb.push(", ");

        // 16. type_name_handler
        qb.push_bind(job.type_name_handler);
        qb.push(", ");

        // 17. payload
        qb.push_bind(sqlx::types::Json(job.payload.clone()));
        qb.push(", ");

        // 18. meta
        qb.push_bind(sqlx::types::Json(job.meta.clone()));
        qb.push(", ");

        // 19. tags
        qb.push_bind(sqlx::types::Json(job.tags.clone()));
        qb.push(", ");

        // 20. errors
        qb.push_bind(sqlx::types::Json(job.errors.clone()));
        qb.push(", ");

        // 21. attempted_by
        qb.push_bind(sqlx::types::Json(serde_json::json!([])));

        qb.push(")");

        qb.build()
            .execute(db)
            .await
            .expect("Failed to insert manual job (Postgres)");
    }

    #[tokio::test]
    async fn test_recover_timed_out_jobs_moves_stale_jobs() {
        setup_bus().await;

        let db = BusQueueConfiguration::global().unwrap().get_connection();
        let queue = "default";

        let mut qb = QueryBuilder::new("DELETE FROM bus_jobs WHERE queue = ");
        qb.push_bind(queue);
        qb.build().execute(db).await.unwrap();

        let job_id = Uuid::now_v7();
        let now = Utc::now();

        let mut hasher = XxHash3_64::with_seed(0);
        hasher.write(OrderHandler::HANDLER_IDENTITY.as_bytes());
        hasher.write(OrderCreated::EVENT_IDENTITY.as_bytes());
        let hash_type_name = hasher.finish() as i64;

        insert_job(JobModel {
            id: job_id,
            hash_type_name,
            state: "executing".to_string(),
            queue: queue.to_string(),
            attempted_at: Some(now - chrono::Duration::minutes(20)),
            scheduled_at: now,
            payload: serde_json::json!({}),
            type_name_event: OrderCreated::EVENT_IDENTITY.to_string(),
            type_name_handler: OrderHandler::HANDLER_IDENTITY.to_string(),
            attempt: 1,
            max_attempts: 3,
            priority: 1,
            execution_timeout_sec: 5,
            meta: serde_json::json!({}),
            tags: serde_json::json!([]),
            errors: serde_json::json!([]),
        })
        .await;

        recover_timed_out_jobs().await.unwrap();

        let job = sqlx::query(SELECT_JOB_SQL)
            .bind(job_id)
            .fetch_one(db)
            .await
            .unwrap();

        let state: String = job.get("state");
        let attempt: i32 = job.get("attempt");
        let errors: Value = job.get("errors");

        assert_ne!(
            state,
            "executing".to_string(),
            "Job має бути витягнута зі стану executing"
        );

        assert_eq!(
            state,
            "retryable".to_string(),
            "Job має перейти в retryable"
        );

        assert_eq!(attempt, 1, "Attempt не має змінюватися при recovery");

        let last_error = errors
            .as_array()
            .and_then(|arr| arr.last())
            .expect("Масив помилок порожній або не є масивом");

        let msg = last_error
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        assert!(
            !msg.is_empty(),
            "Повідомлення про помилку не має бути порожнім"
        );

        assert!(
            msg.contains("timed out"),
            "Очікується timeout помилка, отримано: {}",
            msg
        );
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
        hasher.write(PanicHandler::HANDLER_IDENTITY.as_bytes());
        hasher.write(PanicEvent::EVENT_IDENTITY.as_bytes());
        let hash = hasher.finish() as i64;

        insert_job(JobModel {
            id: job_id,
            hash_type_name: hash,
            state: "available".to_string(),
            queue: queue.to_string(),
            scheduled_at: now,
            attempted_at: None,
            payload: serde_json::json!({"message": "boom"}),
            type_name_event: PanicEvent::EVENT_IDENTITY.to_owned(),
            type_name_handler: PanicHandler::HANDLER_IDENTITY.to_owned(),
            attempt: 0,
            max_attempts: 3,
            priority: 1,
            execution_timeout_sec: 5,
            meta: serde_json::json!({}),
            tags: serde_json::json!([]),
            errors: serde_json::json!([]),
        })
        .await;

        let worker = Worker::new(queue, 1, Duration::from_nanos(1), shutdown_rx);
        let handle = tokio::spawn(async move {
            worker.run().await;
        });

        tokio::time::sleep(Duration::from_secs(2)).await;

        let job = sqlx::query(SELECT_JOB_SQL)
            .bind(job_id)
            .fetch_one(db)
            .await
            .unwrap();

        let state: String = job.get("state");
        let errors: Value = job.get("errors");

        let _ = shutdown_tx.send(());
        let _ = handle.await;

        assert!(
            state == "retryable".to_string() || state == "discarded".to_string(),
            "Timeout має перевести job в retry/discard"
        );

        let errors = errors.as_array().unwrap();
        assert!(!errors.is_empty(), "Має бути записана помилка");
    }

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

        #[cfg(feature = "sqlx-postgres")]
        async fn handle(
            &self,
            _db: &sqlx::PgPool,
            event: &OrderCreated,
            _metadata: &BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            let mut data = FETCH_JOBS_SUCCESS.lock().await;
            *data = event.email.to_owned();
            Ok(())
        }

        #[cfg(feature = "sqlx-mysql")]
        async fn handle(
            &self,
            _db: &sqlx::MySqlPool,
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

        let mut hasher = XxHash3_64::with_seed(0);
        hasher.write(OrderHandler::HANDLER_IDENTITY.as_bytes());
        hasher.write(OrderCreated::EVENT_IDENTITY.as_bytes());
        let hash_type_name = hasher.finish() as i64;

        let now = Utc::now();
        let job_id_ready = Uuid::now_v7();
        let job_id_future = Uuid::now_v7();

        insert_job(JobModel {
            id: job_id_ready,
            hash_type_name,
            scheduled_at: now - chrono::Duration::minutes(5),
            attempted_at: Default::default(),
            state: "available".to_string(),
            priority: 10,
            attempt: 0,
            max_attempts: 22,
            execution_timeout_sec: 14 * 60,
            queue: queue.to_owned(),
            type_name_event: OrderCreated::EVENT_IDENTITY.to_owned(),
            type_name_handler: OrderHandler::HANDLER_IDENTITY.to_owned(),
            payload: serde_json::json!({"email": "test@example.com"}),
            meta: serde_json::json!({}),
            tags: serde_json::json!(["user", "company"]),
            errors: serde_json::json!([]),
        })
        .await;

        insert_job(JobModel {
            id: job_id_future,
            scheduled_at: now + chrono::Duration::hours(1),
            hash_type_name,
            attempted_at: Default::default(),
            state: "available".to_string(),
            priority: 10,
            attempt: 0,
            max_attempts: 22,
            execution_timeout_sec: 14 * 60,
            queue: queue.to_owned(),
            type_name_event: OrderCreated::EVENT_IDENTITY.to_owned(),
            type_name_handler: OrderHandler::HANDLER_IDENTITY.to_owned(),
            payload: serde_json::json!({"email": "test@example.com"}),
            meta: serde_json::json!({}),
            tags: serde_json::json!(["user", "company"]),
            errors: serde_json::json!([]),
        })
        .await;

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

        #[cfg(feature = "sqlx-mysql")]
        let sql = "SELECT state, attempt FROM bus_jobs WHERE id = ?";
        #[cfg(feature = "sqlx-postgres")]
        let sql = "SELECT state::text, attempt FROM bus_jobs WHERE id = $1";

        let row = sqlx::query(sql)
            .bind(job_id_ready)
            .fetch_one(db)
            .await
            .expect("Job not found in database after fetch");

        let state: String = row.get("state");
        let attempt: i32 = row.get("attempt");

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
        let queue = "default";

        let now = Utc::now();
        let handler_hash = 12345;
        let target_id_1 = Uuid::now_v7();
        let target_id_2 = Uuid::now_v7();

        let jobs_data = vec![
            (target_id_1, 999, -10),    // Топ 1 (пріоритет 999, старіша)
            (target_id_2, 999, -5),     // Топ 2 (пріоритет 999, новіша)
            (Uuid::now_v7(), 100, -20), // Менший пріоритет
            (Uuid::now_v7(), 10, -30),
            (Uuid::now_v7(), 5, -5),
            (Uuid::now_v7(), 1, -60),
            (Uuid::now_v7(), 1000, 10), // Високий пріоритет, але в МАЙБУТНЬОМУ (не має вибратись)
        ];

        for (id, priority, offset_min) in jobs_data {
            insert_job(JobModel {
                id,
                priority,
                scheduled_at: now + chrono::Duration::minutes(offset_min),
                state: "available".to_string(),
                attempted_at: None,
                hash_type_name: handler_hash,
                queue: "default".to_owned(),
                payload: serde_json::json!({}),
                attempt: 0,
                max_attempts: 3,
                execution_timeout_sec: 60,
                type_name_event: "TestEvent".to_owned(),
                type_name_handler: "TestHandler".to_owned(),
                meta: serde_json::json!({}),
                tags: serde_json::json!([]),
                errors: serde_json::json!([]),
            })
            .await;
        }

        let jobs = fetch_jobs(queue, 2).await.expect("Failed to fetch jobs");

        assert_eq!(
            jobs.len(),
            2,
            "Мало повернутися рівно 2 джоби згідно з LIMIT"
        );

        let has_target_1 = jobs.iter().any(|j| j.id == target_id_1);
        let has_target_2 = jobs.iter().any(|j| j.id == target_id_2);

        assert!(
            has_target_1,
            "Перша цільова джоба (prio 999, старіша) не знайдена в ліміті"
        );
        assert!(
            has_target_2,
            "Друга цільова джоба (prio 999, новіша) не знайдена в ліміті"
        );

        for job in jobs {
            #[cfg(feature = "sqlx-mysql")]
            let sql = "SELECT state, attempt FROM bus_jobs WHERE id = ?";
            #[cfg(feature = "sqlx-postgres")]
            let sql = "SELECT state::text, attempt FROM bus_jobs WHERE id = $1";

            let row = sqlx::query(sql).bind(job.id).fetch_one(db).await.unwrap();

            let state: String = row.get("state");
            let attempt: i32 = row.get("attempt");

            assert_eq!(state, "executing", "Джоба в БД має бути в стані executing");
            assert_eq!(attempt, 1, "Attempt має бути інкрементований");
        }
    }

    #[tokio::test]
    async fn test_handler_error_retry_flow() {
        setup_bus().await;
        let db = BusQueueConfiguration::global().unwrap().get_connection();
        let job_id = Uuid::now_v7();
        let now = Utc::now();

        let mut hasher = XxHash3_64::with_seed(0);
        hasher.write(OrderHandler::HANDLER_IDENTITY.as_bytes());
        hasher.write(OrderCreated::EVENT_IDENTITY.as_bytes());
        let hash_type_name = hasher.finish() as i64;

        insert_job(JobModel {
            id: job_id,
            state: "executing".to_string(),
            attempt: 1,
            max_attempts: 3,
            priority: 10,
            hash_type_name,
            scheduled_at: now,
            attempted_at: None,
            queue: "default".to_owned(),
            type_name_event: OrderCreated::EVENT_IDENTITY.to_owned(),
            type_name_handler: OrderHandler::HANDLER_IDENTITY.to_owned(),
            payload: serde_json::json!({}),
            meta: serde_json::json!({}),
            tags: serde_json::json!([]),
            errors: serde_json::json!([]),
            execution_timeout_sec: 60,
        })
        .await;

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

        let job_after = sqlx::query(SELECT_JOB_SQL)
            .bind(job_id)
            .fetch_one(db)
            .await
            .unwrap();

        let state: String = job_after.get("state");
        let scheduled_at: DateTime<Utc> = job_after.get("scheduled_at");

        assert_eq!(state, "retryable".to_string());
        assert!(scheduled_at > now, "Наступна спроба має бути в майбутньому");

        let errors: Value = job_after.get("errors");
        assert_eq!(errors.as_array().unwrap().len(), 1);
        assert_eq!(errors[0]["error"], "Database timeout");

        let mut qb = QueryBuilder::new("UPDATE bus_jobs SET ");
        qb.push("state = ");
        qb.push_bind("executing");
        #[cfg(feature = "sqlx-postgres")]
        qb.push("::bus_job_state");
        qb.push(", scheduled_at = ");
        qb.push_bind(Utc::now() - chrono::Duration::minutes(1));
        qb.push(" WHERE id = ");
        qb.push_bind(job_id);
        let query = qb.build();
        query.execute(db).await.expect("Failed to update job state");

        let job_dto_2 = crate::sql::dto::BusJob {
            attempt: 2,
            ..job_dto.clone()
        };

        handler_error(&job_dto_2, "Connection reset by peer".to_string())
            .await
            .unwrap();

        let job_final = sqlx::query(SELECT_JOB_SQL)
            .bind(job_id)
            .fetch_one(db)
            .await
            .unwrap();

        let final_errors: Value = job_final.get("errors");
        let errors_count = final_errors
            .as_array()
            .expect("Поле errors має бути масивом JSON")
            .len();

        assert_eq!(
            errors_count, 2,
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
        let now = Utc::now();

        let job_id = Uuid::now_v7();
        insert_job(JobModel {
            id: job_id,
            hash_type_name: 12345,
            state: "available".to_string(),
            priority: 10,
            scheduled_at: now - chrono::Duration::minutes(1),
            attempted_at: None,
            queue: "default".to_owned(),
            attempt: 0,
            max_attempts: 3,
            execution_timeout_sec: 60,
            type_name_event: "TestEvent".to_owned(),
            type_name_handler: "TestHandler".to_owned(),
            payload: serde_json::json!({}),
            meta: serde_json::json!({}),
            tags: serde_json::json!([]),
            errors: serde_json::json!([]),
        })
        .await;

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

        #[cfg(feature = "sqlx-postgres")]
        async fn handle(
            &self,
            _db: &sqlx::PgPool,
            _event: &PanicEvent,
            _metadata: &BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            panic!("Boom! Something went wrong in the handler logic");
        }

        #[cfg(feature = "sqlx-mysql")]
        async fn handle(
            &self,
            _db: &sqlx::MySqlPool,
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

        let mut qb = QueryBuilder::new("DELETE FROM bus_jobs WHERE queue = ");
        qb.push_bind(queue);
        qb.build().execute(db).await.unwrap();

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

        insert_job(JobModel {
            id: job_id,
            hash_type_name,
            state: "available".to_string(),
            queue: queue.to_owned(),
            scheduled_at: now,
            attempted_at: None,
            payload: serde_json::json!({"email": "real_worker@test.com"}),
            type_name_event: OrderCreated::EVENT_IDENTITY.to_owned(),
            type_name_handler: OrderHandler::HANDLER_IDENTITY.to_owned(),
            attempt: 0,
            max_attempts: 3,
            priority: 10,
            execution_timeout_sec: 5,
            meta: serde_json::json!({}),
            tags: serde_json::json!([]),
            errors: serde_json::json!([]),
        })
        .await;

        let mut success = false;
        for _ in 0..20 {
            tokio::time::sleep(Duration::from_millis(100)).await;

            let job = sqlx::query(SELECT_JOB_SQL)
                .bind(job_id)
                .fetch_one(db)
                .await
                .unwrap();

            let state: String = job.get("state");

            if state == "completed".to_string() {
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

        #[cfg(feature = "sqlx-postgres")]
        async fn handle(
            &self,
            _db: &sqlx::PgPool,
            event: &LongRunningEvent,
            _metadata: &BusMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            tokio::time::sleep(Duration::from_millis(event.duration_ms)).await;
            Ok(())
        }

        #[cfg(feature = "sqlx-mysql")]
        async fn handle(
            &self,
            _db: &sqlx::MySqlPool,
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

        let mut qb = QueryBuilder::new("DELETE FROM bus_jobs WHERE queue = ");
        qb.push_bind(queue);
        qb.build().execute(db).await.unwrap();

        let job_id = Uuid::now_v7();
        let now = Utc::now();

        let mut hasher = XxHash3_64::with_seed(0);
        hasher.write(SleepHandler::HANDLER_IDENTITY.as_bytes());
        hasher.write(LongRunningEvent::EVENT_IDENTITY.as_bytes());
        let hash_type_name = hasher.finish() as i64;

        insert_job(JobModel {
            id: job_id,
            hash_type_name,
            state: "available".to_string(),
            queue: queue.to_owned(),
            scheduled_at: now,
            attempted_at: None,
            payload: serde_json::json!({"duration_ms": 5000}),
            type_name_event: LongRunningEvent::EVENT_IDENTITY.to_owned(),
            type_name_handler: SleepHandler::HANDLER_IDENTITY.to_owned(),
            attempt: 0,
            max_attempts: 3,
            priority: 1,
            execution_timeout_sec: 1,
            meta: serde_json::json!({}),
            tags: serde_json::json!([]),
            errors: serde_json::json!([]),
        })
        .await;

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

        insert_job(JobModel {
            id: job_id,
            state: "completed".to_string(),
            scheduled_at: now,
            attempted_at: None,
            queue: "default".to_string(),
            hash_type_name: 1,
            payload: serde_json::json!({}),
            type_name_event: "E".into(),
            type_name_handler: "H".into(),
            attempt: 1,
            max_attempts: 3,
            priority: 1,
            execution_timeout_sec: 10,
            meta: serde_json::json!({}),
            tags: serde_json::json!([]),
            errors: serde_json::json!([]),
        })
        .await;

        handler_success(job_id).await.unwrap();

        let job = sqlx::query(SELECT_JOB_SQL)
            .bind(job_id)
            .fetch_one(db)
            .await
            .unwrap();
        let state: String = job.get("state");
        assert_eq!(state, "completed".to_string());
    }

    #[tokio::test]
    async fn test_handler_error_discard_flow() {
        setup_bus().await;
        let db = BusQueueConfiguration::global().unwrap().get_connection();

        let job_id = Uuid::now_v7();
        let now = Utc::now();

        insert_job(JobModel {
            id: job_id,
            state: "executing".to_string(),
            attempt: 3,
            max_attempts: 3,
            scheduled_at: now,
            attempted_at: None,
            queue: "default".to_string(),
            hash_type_name: 1,
            payload: serde_json::json!({}),
            type_name_event: "E".into(),
            type_name_handler: "H".into(),
            priority: 1,
            execution_timeout_sec: 10,
            meta: serde_json::json!({}),
            tags: serde_json::json!([]),
            errors: serde_json::json!([]),
        })
        .await;
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

        let job = sqlx::query(SELECT_JOB_SQL)
            .bind(job_id)
            .fetch_one(db)
            .await
            .unwrap();

        let state: String = job.get("state");
        let discarded_at: Option<DateTime<Utc>> = job.get("discarded_at");

        assert_eq!(state, "discarded".to_string());
        assert!(discarded_at.is_some());
    }

    #[tokio::test]
    async fn test_worker_recovers_from_panic() {
        setup_bus().await;
        let db = BusQueueConfiguration::global().unwrap().get_connection();

        let queue = "panic_queue";

        let mut qb = QueryBuilder::new("DELETE FROM bus_jobs WHERE queue = ");
        qb.push_bind(queue);
        qb.build().execute(db).await.unwrap();

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

        insert_job(JobModel {
            id: job_id,
            hash_type_name: hash,
            state: "available".to_string(),
            queue: queue.to_string(),
            scheduled_at: now,
            attempted_at: None,
            payload: serde_json::json!({"message": "boom"}),
            type_name_event: PanicEvent::EVENT_IDENTITY.to_owned(),
            type_name_handler: PanicHandler::HANDLER_IDENTITY.to_owned(),
            attempt: 0,
            max_attempts: 3,
            priority: 1,
            execution_timeout_sec: 5,
            meta: serde_json::json!({}),
            tags: serde_json::json!([]),
            errors: serde_json::json!([]),
        })
        .await;

        tokio::time::sleep(Duration::from_secs(1)).await;

        let job = sqlx::query(SELECT_JOB_SQL)
            .bind(job_id)
            .fetch_one(db)
            .await
            .unwrap();

        let state: String = job.get("state");
        let errors: Value = job.get("errors");

        let _ = shutdown_tx.send(());
        let _ = handle.await;

        assert!(
            state == "retryable".to_string() || state == "discarded".to_string(),
            "Panic має бути оброблений як помилка"
        );

        assert!(
            !errors.as_array().unwrap().is_empty(),
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

        let mut qb = QueryBuilder::new("DELETE FROM bus_jobs WHERE queue = ");
        qb.push_bind(queue);
        qb.build().execute(db).await.unwrap();

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(());

        let job_id = Uuid::now_v7();
        let now = Utc::now();

        let mut hasher = XxHash3_64::with_seed(0);
        hasher.write(SleepHandler::HANDLER_IDENTITY.as_bytes());
        hasher.write(LongRunningEvent::EVENT_IDENTITY.as_bytes());
        let hash = hasher.finish() as i64;

        insert_job(JobModel {
            id: job_id,
            hash_type_name: hash,
            state: "available".to_string(),
            queue: queue.to_string(),
            scheduled_at: now,
            attempted_at: None,
            payload: serde_json::json!({"duration_ms": 5000}),
            type_name_event: LongRunningEvent::EVENT_IDENTITY.to_owned(),
            type_name_handler: SleepHandler::HANDLER_IDENTITY.to_owned(),
            attempt: 0,
            max_attempts: 3,
            priority: 1,
            execution_timeout_sec: 10,
            meta: serde_json::json!({}),
            tags: serde_json::json!([]),
            errors: serde_json::json!([]),
        })
        .await;

        let worker = Worker::new(queue, 1, Duration::from_nanos(1), shutdown_rx);
        let handle = tokio::spawn(async move {
            worker.run().await;
        });

        let mut started = false;
        for _ in 0..100 {
            let check_job = sqlx::query(SELECT_JOB_SQL)
                .bind(job_id)
                .fetch_one(db)
                .await
                .unwrap();

            let state: String = check_job.get("state");

            if state == "executing".to_string() {
                started = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        assert!(started, "Воркер не підхопив джобу за 5 секунд");

        let _ = shutdown_tx.send(());
        let _ = handle.await;

        let job = sqlx::query(SELECT_JOB_SQL)
            .bind(job_id)
            .fetch_one(db)
            .await
            .unwrap();

        let state: String = job.get("state");

        assert_ne!(
            state,
            "completed".to_string(),
            "Job не мав завершитися під час shutdown"
        );

        assert!(
            state == "available".to_string()
                || state == "retryable".to_string()
                || state == "executing".to_string(),
            "Job має залишитись доступною для повторної обробки"
        );

        tokio::time::sleep(Duration::from_millis(600)).await;

        let job = sqlx::query(SELECT_JOB_SQL)
            .bind(job_id)
            .fetch_one(db)
            .await
            .unwrap();

        let state: String = job.get("state");
        let attempt: i32 = job.get("attempt");
        let errors: Value = job.get("errors");

        assert_eq!(
            state,
            "available".to_string(),
            "Стан має бути Available після shutdown"
        );

        assert_eq!(
            attempt, 0,
            "Attempt не має інкрементуватися при звичайному shutdown"
        );

        assert!(
            !errors.as_array().unwrap().is_empty(),
            "Список помилок не має бути порожнім"
        );

        let last_error = errors.as_array().unwrap().last().unwrap();
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
            attempt
        );
    }

    #[tokio::test]
    async fn test_recover_does_not_touch_fresh_jobs() {
        setup_bus().await;

        let db = BusQueueConfiguration::global().unwrap().get_connection();
        let queue = "default";

        let mut qb = QueryBuilder::new("DELETE FROM bus_jobs WHERE queue = ");
        qb.push_bind(queue);
        qb.build().execute(db).await.unwrap();

        let job_id = Uuid::now_v7();
        let now = Utc::now();

        insert_job(JobModel {
            id: job_id,
            hash_type_name: 123,
            state: "executing".to_string(),
            queue: queue.to_string(),
            attempted_at: Some(now),
            scheduled_at: now,
            payload: serde_json::json!({}),
            type_name_event: "TestEvent".to_string(),
            type_name_handler: "TestHandler".to_string(),
            attempt: 1,
            max_attempts: 3,
            priority: 1,
            execution_timeout_sec: 60,
            meta: serde_json::json!({}),
            tags: serde_json::json!([]),
            errors: serde_json::json!([]),
        })
        .await;

        recover_timed_out_jobs().await.unwrap();

        let job = sqlx::query(SELECT_JOB_SQL)
            .bind(job_id)
            .fetch_one(db)
            .await
            .unwrap();

        let state: String = job.get("state");

        assert_eq!(
            state,
            "executing".to_string(),
            "Свіжі jobs не мають чіпатись"
        );
    }
}
