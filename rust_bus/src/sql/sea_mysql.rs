use crate::BusError;
use crate::contracts::meta::BusMetadata;
use crate::dispatch::registration::DATABASE_HANDLERS_BY_HASH;
use crate::sql::dto::BusJob;
use crate::workers::configuration::BusQueueConfiguration;
use chrono::Utc;
use futures::future::join_all;
use sea_orm::TransactionTrait;
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde_json::Value;
use uuid::Uuid;

pub(crate) async fn fetch_jobs(queue_name: &str, limit: usize) -> Result<Vec<BusJob>, BusError> {
    let db = BusQueueConfiguration::global()?.get_connection();
    let txn = db.begin().await?;
    let now = Utc::now();

    let select_sql = r#"
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

    let select_stmt = Statement::from_sql_and_values(
        DbBackend::MySql,
        select_sql,
        vec![queue_name.into(), now.into(), (limit as i64).into()],
    );

    let rows = txn.query_all_raw(select_stmt).await?;

    if rows.is_empty() {
        txn.commit().await?;
        return Ok(vec![]);
    }

    let mut jobs = Vec::with_capacity(rows.len());
    let mut ids = Vec::with_capacity(rows.len());

    for row in rows {
        let id: Uuid = row.try_get("", "id")?;
        ids.push(id);

        let meta_json: Value = row.try_get("", "meta")?;
        let meta: BusMetadata = serde_json::from_value(meta_json)
            .map_err(|e| sea_orm::DbErr::Custom(format!("Meta conversion error: {}", e)))?;

        jobs.push(BusJob {
            id,
            hash_type_name: row.try_get("", "hash_type_name")?,
            attempt: row.try_get("", "attempt")?,
            max_attempts: row.try_get("", "max_attempts")?,
            execution_timeout_sec: row.try_get("", "execution_timeout_sec")?,
            type_name_event: row.try_get("", "type_name_event")?,
            type_name_handler: row.try_get("", "type_name_handler")?,
            payload: row.try_get("", "payload")?,
            meta,
            tags: row.try_get("", "tags")?,
            errors: row.try_get("", "errors")?,
        });
    }

    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let update_sql = format!(
        "UPDATE bus_jobs SET state = 'executing', attempted_at = ?, attempt = attempt + 1 WHERE id IN ({})",
        placeholders
    );

    let mut update_values: Vec<sea_orm::Value> = Vec::with_capacity(ids.len());
    update_values.push(Utc::now().into());
    for id in ids {
        update_values.push(id.into());
    }

    let update_stmt = Statement::from_sql_and_values(DbBackend::MySql, &update_sql, update_values);

    txn.execute_raw(update_stmt).await?;
    txn.commit().await?;

    Ok(jobs)
}

pub(crate) async fn handler_success(id: Uuid) -> Result<(), BusError> {
    let pool = BusQueueConfiguration::global()?.get_connection();

    let stmt = Statement::from_sql_and_values(
        DbBackend::MySql,
        r#"
        UPDATE bus_jobs
            SET state = 'completed',
            completed_at = ?
        WHERE id = ?
          AND state = 'executing'
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

    let err_json = serde_json::json!({
        "attempt": job.attempt,
        "error": err_display,
        "at": Utc::now().to_rfc3339()
    });

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
            SET state = 'retryable',
                scheduled_at = ?,
                errors = JSON_ARRAY_APPEND(COALESCE(errors, CAST('[]' AS JSON)), '$', CAST(? AS JSON))
            WHERE id = ?
              AND state = 'executing'
        "#;

        (
            sql,
            vec![
                scheduled_at.into(),
                err_json.to_string().into(),
                job.id.into(),
            ],
        )
    } else {
        let sql = r#"
            UPDATE bus_jobs
            SET state = 'discarded',
                discarded_at = ?,
                errors = JSON_ARRAY_APPEND(COALESCE(errors, CAST('[]' AS JSON)), '$', CAST(? AS JSON))
            WHERE id = ?
              AND state = 'executing'
        "#;

        (
            sql,
            vec![
                Utc::now().into(),
                err_json.to_string().into(),
                job.id.into(),
            ],
        )
    };

    let stmt = Statement::from_sql_and_values(DbBackend::MySql, sql, values);
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
                WHEN (attempt - 1) > 0 THEN 'retryable'
                ELSE 'available'
            END,
            attempt = attempt - 1,
            errors = JSON_ARRAY_APPEND(IFNULL(errors, CAST('[]' AS JSON)), '$', CAST(? AS JSON))
        WHERE id = ?
          AND state = 'executing'
    "#;

    let err_json = serde_json::json!({
        "attempt": job.attempt,
        "error": "Worker shutdown: job interrupted",
        "at": Utc::now().to_rfc3339()
    });

    db.execute_raw(Statement::from_sql_and_values(
        DbBackend::MySql,
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
        DbBackend::MySql,
        r#"
            SELECT
                id, hash_type_name, attempt, max_attempts,
                execution_timeout_sec, type_name_event, type_name_handler,
                payload, meta, tags, errors
            FROM bus_jobs
            WHERE state = 'executing'
              AND DATE_ADD(attempted_at, INTERVAL execution_timeout_sec SECOND) < ?
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
        let meta_json: Value = row.try_get("", "meta")?;
        let meta: BusMetadata = serde_json::from_value(meta_json)
            .map_err(|e| sea_orm::DbErr::Custom(format!("Meta conversion error: {}", e)))?;

        let job = BusJob {
            id,
            hash_type_name: row.try_get("", "hash_type_name")?,
            attempt: row.try_get("", "attempt")?,
            max_attempts: row.try_get("", "max_attempts")?,
            execution_timeout_sec: row.try_get("", "execution_timeout_sec")?,
            type_name_event: row.try_get("", "type_name_event")?,
            type_name_handler: row.try_get("", "type_name_handler")?,
            payload: row.try_get("", "payload")?,
            meta,
            tags: row.try_get("", "tags")?,
            errors: row.try_get("", "errors")?,
        };

        let fut = async move {
            let timeout_sec = job.execution_timeout_sec;
            if let Err(_e) = crate::sql::handler_error(
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
