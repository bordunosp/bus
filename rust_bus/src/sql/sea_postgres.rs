use crate::BusError;
use crate::contracts::meta::BusMetadata;
use crate::dispatch::registration::DATABASE_HANDLERS_BY_HASH;
use crate::sql::dto::BusJob;
use crate::workers::configuration::BusQueueConfiguration;
use chrono::Utc;
use futures::future::join_all;
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde_json::Value;
use uuid::Uuid;

pub(crate) async fn fetch_jobs(queue_name: &str, limit: usize) -> Result<Vec<BusJob>, BusError> {
    let db = BusQueueConfiguration::global()?.get_connection();
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

    let stmt = Statement::from_sql_and_values(
        DbBackend::Postgres,
        sql,
        vec![
            queue_name.into(),
            now.into(),
            (limit as i64).into(),
            now.into(),
        ],
    );

    let rows = db.query_all_raw(stmt).await?;
    let mut jobs = Vec::with_capacity(rows.len());

    for row in rows {
        let id: Uuid = row.try_get("", "id")?;
        let attempt: i32 = row.try_get("", "attempt")?;
        let max_attempts: i32 = row.try_get("", "max_attempts")?;
        let execution_timeout_sec: i32 = row.try_get("", "execution_timeout_sec")?;
        let type_name_event: String = row.try_get("", "type_name_event")?;
        let type_name_handler: String = row.try_get("", "type_name_handler")?;

        let payload: Value = row.try_get("", "payload")?;
        let tags: Value = row.try_get("", "tags")?;
        let errors: Value = row.try_get("", "errors")?;

        let meta_json: Value = row.try_get("", "meta")?;
        let meta: BusMetadata = serde_json::from_value(meta_json)
            .map_err(|e| sea_orm::DbErr::Custom(format!("Meta conversion error: {}", e)))?;

        jobs.push(BusJob {
            id,
            hash_type_name: row.try_get("", "hash_type_name")?,
            attempt,
            max_attempts,
            execution_timeout_sec,
            type_name_event,
            type_name_handler,
            payload,
            meta,
            tags,
            errors,
        });
    }

    Ok(jobs)
}

pub(crate) async fn handler_success(id: Uuid) -> Result<(), BusError> {
    let pool = BusQueueConfiguration::global()?.get_connection();

    let stmt = Statement::from_sql_and_values(
        DbBackend::Postgres,
        r#"
        UPDATE bus_jobs
            SET state = 'completed'::bus_job_state,
            completed_at = $1
        WHERE id = $2
          AND state = 'executing'::bus_job_state
        "#,
        vec![Utc::now().into(), id.into()],
    );

    let _result = pool
        .execute_raw(stmt)
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
        "at": Utc::now().to_rfc3339()
    }]);

    let (sql, values) = if job.attempt < job.max_attempts {
        let database_handlers = DATABASE_HANDLERS_BY_HASH.get().ok_or_else(|| {
            BusError::Configuration("Bus not initialized. Call Bus::init().".into())
        })?;

        let reg = database_handlers.get(&job.hash_type_name).ok_or_else(|| {
            BusError::Configuration(format!("Handler not found: {}", job.hash_type_name))
        })?;

        let delay = (reg.next_attempt_at)(job.attempt as u32);
        let scheduled_at = Utc::now() + delay;

        let sql = r#"
            UPDATE bus_jobs
            SET state = 'retryable'::bus_job_state,
                scheduled_at = $1,
                errors = COALESCE(errors, '[]'::jsonb) || $2::jsonb
            WHERE id = $3
              AND state = 'executing'::bus_job_state
        "#;

        (
            sql,
            vec![scheduled_at.into(), err_json.into(), job.id.into()],
        )
    } else {
        let sql = r#"
            UPDATE bus_jobs
            SET state = 'discarded'::bus_job_state,
                discarded_at = $1,
                errors = COALESCE(errors, '[]'::jsonb) || $2::jsonb
            WHERE id = $3
              AND state = 'executing'::bus_job_state
        "#;

        (sql, vec![Utc::now().into(), err_json.into(), job.id.into()])
    };

    let stmt = Statement::from_sql_and_values(DbBackend::Postgres, sql, values);

    let _result = pool
        .execute_raw(stmt)
        .await
        .map_err(|e| BusError::Database(e.to_string()))?;

    #[cfg(feature = "logging")]
    if _result.rows_affected() == 0 {
        log::warn!("[Worker] handler_error skipped (stale job): {}", job.id);
    }

    Ok(())
}

pub(crate) async fn handler_error_shutdown(job: &BusJob) -> Result<(), BusError> {
    let db = BusQueueConfiguration::global()?.get_connection();

    let sql = r#"
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
    "#;

    let err_json = serde_json::json!([{
        "attempt": job.attempt,
        "error": "Worker shutdown: job interrupted",
        "at": Utc::now().to_rfc3339()
    }]);

    db.execute_raw(Statement::from_sql_and_values(
        DbBackend::Postgres,
        sql,
        vec![err_json.into(), job.id.into()],
    ))
    .await
    .map_err(|e| BusError::Database(e.to_string()))?;

    Ok(())
}

pub async fn recover_timed_out_jobs() -> Result<(), BusError> {
    let db = BusQueueConfiguration::global()?.get_connection();

    let select_stmt = Statement::from_sql_and_values(
        DbBackend::Postgres,
        r#"
            SELECT
                id, hash_type_name, attempt, max_attempts,
                execution_timeout_sec, type_name_event, type_name_handler,
                payload, meta, tags, errors
            FROM bus_jobs
            WHERE state = 'executing'::bus_job_state
              AND attempted_at + (interval '1 second' * execution_timeout_sec) < $1
            LIMIT 100
        "#,
        vec![Utc::now().into()],
    );

    let rows = db.query_all_raw(select_stmt).await?;

    if rows.is_empty() {
        return Ok(());
    }

    let mut futures = Vec::new();

    for row in rows {
        let id: Uuid = row.try_get("", "id")?;
        let attempt: i32 = row.try_get("", "attempt")?;
        let max_attempts: i32 = row.try_get("", "max_attempts")?;
        let execution_timeout_sec: i32 = row.try_get("", "execution_timeout_sec")?;
        let type_name_event: String = row.try_get("", "type_name_event")?;
        let type_name_handler: String = row.try_get("", "type_name_handler")?;

        let payload: Value = row.try_get("", "payload")?;
        let tags: Value = row.try_get("", "tags")?;
        let errors: Value = row.try_get("", "errors")?;

        let meta_json: Value = row.try_get("", "meta")?;
        let meta: BusMetadata = serde_json::from_value(meta_json)
            .map_err(|e| sea_orm::DbErr::Custom(format!("Meta conversion error: {}", e)))?;

        let job = BusJob {
            id,
            hash_type_name: row.try_get("", "hash_type_name")?,
            attempt,
            max_attempts,
            execution_timeout_sec,
            type_name_event,
            type_name_handler,
            payload,
            meta,
            tags,
            errors,
        };

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
