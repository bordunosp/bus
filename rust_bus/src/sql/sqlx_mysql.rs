use crate::BusError;
use crate::contracts::meta::BusMetadata;
use crate::dispatch::registration::DATABASE_HANDLERS_BY_HASH;
use crate::sql::dto::BusJob;
use crate::workers::configuration::BusQueueConfiguration;
use chrono::Utc;
use futures::future::join_all;
use serde_json::Value;
use sqlx::MySql;
use sqlx::QueryBuilder;
use sqlx::{FromRow, Row, mysql::MySqlRow};
use uuid::Uuid;

impl<'r> FromRow<'r, MySqlRow> for BusJob {
    fn from_row(row: &'r MySqlRow) -> Result<Self, sqlx::Error> {
        let id_bytes: Vec<u8> = row.try_get("id")?;
        let id = Uuid::from_slice(&id_bytes).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;

        let payload: Value = row.try_get("payload")?;
        let tags: Value = row.try_get("tags")?;
        let errors: Value = row.try_get("errors")?;

        let meta_value: Value = row.try_get("meta")?;
        let meta: BusMetadata =
            serde_json::from_value(meta_value).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;

        Ok(Self {
            id,
            hash_type_name: row.try_get("hash_type_name")?,
            attempt: row.try_get("attempt")?,
            max_attempts: row.try_get("max_attempts")?,
            execution_timeout_sec: row.try_get("execution_timeout_sec")?,
            type_name_event: row.try_get("type_name_event")?,
            type_name_handler: row.try_get("type_name_handler")?,
            payload,
            meta,
            tags,
            errors,
        })
    }
}

pub(crate) async fn fetch_jobs(queue_name: &str, limit: usize) -> Result<Vec<BusJob>, BusError> {
    let pool = BusQueueConfiguration::global()?.get_connection();
    let mut txn = pool.begin().await?;
    let now = Utc::now();

    let sql = r#"
        SELECT
            id, hash_type_name, attempt + 1 as attempt, max_attempts, execution_timeout_sec,
            type_name_event, type_name_handler, payload, meta, tags, errors
        FROM bus_jobs USE INDEX(idx_bus_jobs_fetch_available)
        WHERE queue = ?
          AND state IN ('available', 'retryable')
          AND scheduled_at <= ?
        ORDER BY priority DESC, scheduled_at ASC, id ASC
        LIMIT ?
        FOR UPDATE SKIP LOCKED
    "#;

    let jobs = sqlx::query_as::<_, BusJob>(sql)
        .bind(queue_name)
        .bind(now)
        .bind(limit as i64)
        .fetch_all(&mut *txn)
        .await?;

    if jobs.is_empty() {
        txn.commit().await?;
        return Ok(vec![]);
    }

    let mut query_builder: QueryBuilder<MySql> =
        QueryBuilder::new("UPDATE bus_jobs SET state = 'executing', attempted_at = ");

    query_builder.push_bind(Utc::now());
    query_builder.push(", attempt = attempt + 1 WHERE id IN (");

    let mut separated = query_builder.separated(", ");
    for id in jobs.iter().map(|j| j.id.as_bytes()) {
        separated.push_bind(&id[..]);
    }
    separated.push_unseparated(")");

    query_builder.build().execute(&mut *txn).await?;
    txn.commit().await?;
    Ok(jobs)
}

pub(crate) async fn handler_success(id: Uuid) -> Result<(), BusError> {
    let pool = BusQueueConfiguration::global()?.get_connection();

    let _result = sqlx::query(
        r#"
        UPDATE bus_jobs
            SET state = 'completed',
            completed_at = ?
        WHERE id = ?
          AND state = 'executing'
        "#,
    )
    .bind(Utc::now())
    .bind(&id.as_bytes()[..])
    .execute(pool)
    .await
    .map_err(|e| BusError::Database(e.to_string()))?;

    #[cfg(feature = "logging")]
    if _result.rows_affected() == 0 {
        log::warn!("[Worker] handler_success skipped (stale job): {}", id);
    }

    Ok(())
}

pub(crate) async fn handler_error(job: &BusJob, err_display: String) -> Result<(), BusError> {
    let pool = BusQueueConfiguration::global()?.get_connection();

    let err_json = serde_json::json!({
        "attempt": job.attempt,
        "error": err_display,
        "at": chrono::Utc::now().to_rfc3339()
    });

    if job.attempt < job.max_attempts {
        let database_handlers = DATABASE_HANDLERS_BY_HASH.get().ok_or_else(|| {
            BusError::Configuration(
                "Bus not initialized. Call Bus::init() before dispatching.".to_string(),
            )
        })?;

        let reg = database_handlers.get(&job.hash_type_name).ok_or_else(|| {
            BusError::Configuration(format!(
                "Handler not found by handler {} and event {}",
                job.hash_type_name, job.type_name_event,
            ))
        })?;

        let delay = (reg.next_attempt_at)(job.attempt as u32);

        let _result = sqlx::query(
            r#"
                UPDATE bus_jobs
                SET state = 'retryable',
                    scheduled_at = ?,
                    errors = JSON_ARRAY_APPEND(
                        COALESCE(errors, CAST('[]' AS JSON)),
                        '$',
                        CAST(? AS JSON)
                    )
                WHERE id = ?
                  AND state = 'executing'
            "#,
        )
        .bind(chrono::Utc::now() + delay)
        .bind(err_json)
        .bind(&job.id.as_bytes()[..])
        .execute(pool)
        .await
        .map_err(|e| BusError::Database(e.to_string()))?;

        #[cfg(feature = "logging")]
        if _result.rows_affected() == 0 {
            log::warn!(
                "[Worker] handler_error skipped retry (stale job): {}",
                job.id
            );
        }
    } else {
        let _result = sqlx::query(
            r#"
                UPDATE bus_jobs
                SET state = 'discarded',
                    discarded_at = ?,
                    errors = JSON_ARRAY_APPEND(
                        COALESCE(errors, CAST('[]' AS JSON)),
                        '$',
                        CAST(? AS JSON)
                    )
                WHERE id = ?
                  AND state = 'executing'
            "#,
        )
        .bind(chrono::Utc::now())
        .bind(err_json)
        .bind(&job.id.as_bytes()[..])
        .execute(pool)
        .await
        .map_err(|e| BusError::Database(e.to_string()))?;

        #[cfg(feature = "logging")]
        if _result.rows_affected() == 0 {
            log::warn!(
                "[Worker] handler_error skipped discard (stale job): {}",
                job.id
            );
        }
    };

    Ok(())
}

pub(crate) async fn handler_error_shutdown(job: &BusJob) -> Result<(), BusError> {
    let pool = BusQueueConfiguration::global()?.get_connection();

    let err_json = serde_json::json!({
        "attempt": job.attempt,
        "error": "Worker shutdown: job interrupted",
        "at": Utc::now().to_rfc3339()
    });

    sqlx::query!(
        r#"
        UPDATE bus_jobs
        SET
            state = CASE
                WHEN (attempt - 1) > 0 THEN 'retryable'
                ELSE 'available'
            END,
            attempt = attempt - 1,
            errors = JSON_ARRAY_APPEND(IFNULL(errors, CAST('[]' AS JSON)), '$', CAST(? AS JSON))
        WHERE id = ?
          AND state = 'executing'
        "#,
        err_json,
        job.id
    )
    .execute(pool)
    .await
    .map_err(|e| BusError::Database(e.to_string()))?;

    Ok(())
}

pub async fn recover_timed_out_jobs() -> Result<(), BusError> {
    let pool = BusQueueConfiguration::global()?.get_connection();

    let sql = r#"
            SELECT
                id, hash_type_name, attempt, max_attempts,
                execution_timeout_sec, type_name_event, type_name_handler,
                payload, meta, tags, errors
            FROM bus_jobs
            WHERE state = 'executing'
              AND DATE_ADD(attempted_at, INTERVAL execution_timeout_sec SECOND) < ?
            LIMIT 100
        "#;

    let jobs = sqlx::query_as::<_, BusJob>(sql)
        .bind(Utc::now())
        .fetch_all(pool)
        .await?;

    if jobs.is_empty() {
        return Ok(());
    }

    let mut futures = Vec::new();

    for job in jobs.iter() {
        let fut = async move {
            let timeout_sec = job.execution_timeout_sec;
            if let Err(_e) = handler_error(
                &job,
                format!("Job execution timed out after {}s", timeout_sec),
            )
            .await
            {
                #[cfg(feature = "logging")]
                log::error!("[Recovery] Failed to recover job {}: {:?}", job.id, _e);
            }
        };

        futures.push(fut);
    }

    join_all(futures).await;
    Ok(())
}
