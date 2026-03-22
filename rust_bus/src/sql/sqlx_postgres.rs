use crate::BusError;
use crate::contracts::meta::BusMetadata;
use crate::dispatch::registration::DATABASE_HANDLERS_BY_HASH;
use crate::sql::dto::BusJob;
use crate::workers::configuration::BusQueueConfiguration;
use chrono::Utc;
use futures::future::join_all;
use serde_json::Value;
use sqlx::{FromRow, Row, postgres::PgRow};
use uuid::Uuid;

impl<'r> FromRow<'r, PgRow> for BusJob {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let id: Uuid = row.try_get("id")?;

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
    let now = Utc::now();

    let sql = r#"
        WITH selected AS (
            SELECT id
            FROM bus_jobs
            WHERE queue = $1
              AND state IN ('available'::bus_job_state, 'retryable'::bus_job_state)
              AND scheduled_at <= $2
            ORDER BY priority DESC, scheduled_at ASC, id ASC
            LIMIT $3
            FOR UPDATE SKIP LOCKED
        )
        UPDATE bus_jobs
        SET
            state = 'executing'::bus_job_state,
            attempted_at = $4,
            attempt = attempt + 1
        FROM selected
        WHERE bus_jobs.id = selected.id
        RETURNING
            bus_jobs.id, bus_jobs.hash_type_name, bus_jobs.attempt, bus_jobs.max_attempts,
            bus_jobs.execution_timeout_sec, bus_jobs.type_name_event, bus_jobs.type_name_handler,
            bus_jobs.payload, bus_jobs.meta, bus_jobs.tags, bus_jobs.errors
    "#;

    let jobs = sqlx::query_as::<_, BusJob>(sql)
        .bind(queue_name)
        .bind(now)
        .bind(limit as i64)
        .bind(now)
        .fetch_all(pool)
        .await?;

    Ok(jobs)
}

pub(crate) async fn handler_success(id: Uuid) -> Result<(), BusError> {
    let pool = BusQueueConfiguration::global()?.get_connection();

    let _result = sqlx::query(
        r#"
        UPDATE bus_jobs
            SET state = 'completed'::bus_job_state,
            completed_at = $1
        WHERE id = $2
          AND state = 'executing'::bus_job_state
        "#,
    )
    .bind(Utc::now())
    .bind(id)
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

    let err_json = serde_json::json!([{
        "attempt": job.attempt,
        "error": err_display,
        "at": chrono::Utc::now().to_rfc3339()
    }]);

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
                SET state = 'retryable'::bus_job_state,
                    scheduled_at = $1,
                    errors = COALESCE(errors, '[]'::jsonb) || $2::jsonb
                WHERE id = $3
                  AND state = 'executing'::bus_job_state
            "#,
        )
        .bind(chrono::Utc::now() + delay)
        .bind(err_json)
        .bind(job.id)
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
                SET state = 'discarded'::bus_job_state,
                    discarded_at = $1,
                    errors = COALESCE(errors, '[]'::jsonb) || $2::jsonb
                WHERE id = $3
                  AND state = 'executing'::bus_job_state
            "#,
        )
        .bind(chrono::Utc::now())
        .bind(err_json)
        .bind(job.id)
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

    let err_json = serde_json::json!([{
        "attempt": job.attempt,
        "error": "Worker shutdown: job interrupted",
        "at": Utc::now().to_rfc3339()
    }]);

    sqlx::query!(
        r#"
        UPDATE bus_jobs
        SET
            state = CASE
                WHEN (attempt - 1) > 0 THEN 'retryable'::bus_job_state
                ELSE 'available'::bus_job_state
            END,
            attempt = attempt - 1,
            errors = COALESCE(errors, '[]'::jsonb) || $1::jsonb
        WHERE id = $2
          AND state = 'executing'::bus_job_state
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
            WHERE state = 'executing'::bus_job_state
              AND attempted_at + (interval '1 second' * execution_timeout_sec) < $1
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
