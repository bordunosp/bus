use crate::core::error_bus::BusError;
use crate::core::features::sea_orm::core::{ArchiveType, DatabaseQueueConfiguration};
use crate::core::features::sea_orm::db_enums::EventStatusEnum;
use crate::core::features::sea_orm::dto::{
    BusEvent, BusEventForArchive, BusEventJob, IdArchiveJob,
};
use crate::core::features::sea_orm::initialization::{get_db_conn, get_queue_config};
use chrono::Utc;
use sea_orm::{
    ConnectionTrait, DatabaseTransaction, DbBackend, Statement, TransactionTrait, Value,
};
use std::error::Error;
use uuid::Uuid;

pub(crate) async fn get_expires() -> Result<Vec<BusEventJob>, Box<dyn Error + Send + Sync>> {
    let table_name = get_queue_config()?.connection().table_name();

    let query = Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Postgres,
        format!(
            r#"
            SELECT
              id, queue_name, type_name_event, type_name_handler, payload_bin, 
              retries_current, retries_max, archive_mode::TEXT, expires_at,
              EXTRACT(EPOCH FROM expires_interval)::BIGINT AS expires_interval
            FROM {table_name}
            WHERE status = $1::bus_events_status_enum
              AND expires_at > $2
            ORDER BY expires_at ASC
            LIMIT 10
            "#
        ),
        vec![
            Value::String(Some(EventStatusEnum::Processing.to_string())),
            Value::ChronoDateTime(Some(Utc::now().naive_utc() + chrono::Duration::minutes(1))),
        ],
    );

    get_db_conn()?
        .query_all_raw(query)
        .await?
        .into_iter()
        .map(|row| {
            Ok(BusEventJob {
                id: row.try_get("", "id")?,
                type_name_event: row.try_get("", "type_name_event")?,
                type_name_handler: row.try_get("", "type_name_handler")?,
                payload_bin: row.try_get("", "payload_bin")?,
                retries_current: row.try_get("", "retries_current")?,
                retries_max: row.try_get("", "retries_max")?,
                archive_mode: {
                    let s: String = row.try_get("", "archive_mode")?;
                    ArchiveType::try_from(s)?
                },
                expires_interval: {
                    let seconds: i64 = row.try_get("", "expires_interval")?;
                    std::time::Duration::from_secs(seconds as u64)
                },
            })
        })
        .collect::<Result<Vec<BusEventJob>, Box<dyn Error + Send + Sync>>>()
}

pub(crate) async fn get_id_status(
    status: EventStatusEnum,
) -> Result<Vec<IdArchiveJob>, Box<dyn Error + Send + Sync>> {
    let table_name = get_queue_config()?.connection().table_name();

    get_db_conn()?
        .query_all_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            format!(
                r#"
                SELECT id, archive_mode::TEXT
                FROM {table_name}
                WHERE status = $1::bus_events_status_enum
                LIMIT 10
                "#
            ),
            vec![Value::String(Some(status.to_string()))],
        ))
        .await?
        .into_iter()
        .map(|row| {
            Ok(IdArchiveJob {
                id: row.try_get("", "id")?,
                archive_mode: {
                    let s: String = row.try_get("", "archive_mode")?;
                    ArchiveType::try_from(s)?
                },
            })
        })
        .collect::<Result<Vec<IdArchiveJob>, Box<dyn Error + Send + Sync>>>()
}

pub(crate) async fn delete(id: Uuid) -> Result<(), Box<dyn Error + Send + Sync>> {
    let table_name = get_queue_config()?.connection().table_name();

    get_db_conn()?
        .execute_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            format!(r#"DELETE FROM {table_name} WHERE id = $1;"#),
            vec![Value::Uuid(Some(id))],
        ))
        .await?;
    Ok(())
}

pub(crate) async fn update_scheduled_at(
    id: Uuid,
    scheduled_at: chrono::Duration,
    err: Box<dyn Error + Send + Sync>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let table_name = get_queue_config()?.connection().table_name();

    get_db_conn()?
        .execute_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            format!(
                r#"
                UPDATE {table_name}
                SET status = $1::bus_events_status_enum,
                    retries_current = retries_current + 1,
                    latest_error = $2,
                    expires_at = $3,
                    should_start_at = $4,
                    updated_at = (NOW() AT TIME ZONE 'UTC')
                WHERE id = $5
                "#
            ),
            vec![
                Value::String(Some(EventStatusEnum::Pending.to_string())),
                Value::String(Some(err.to_string())),
                Value::ChronoDateTime(None),
                Value::ChronoDateTime(Some(Utc::now().naive_utc() + scheduled_at)),
                Value::Uuid(Some(id)),
            ],
        ))
        .await?;

    Ok(())
}

pub(crate) async fn archive(id: Uuid) -> Result<(), BusError> {
    let db = get_db_conn()?;
    let txn = db.begin().await?;
    let table_name = get_queue_config()?.connection().table_name();
    let table_name_archive = get_queue_config()?.connection().table_name_archive();

    let select_stmt = Statement::from_sql_and_values(
        DbBackend::Postgres,
        format!(
            r#"
            SELECT 
              id, queue_name, type_name_event, type_name_handler, 
              status::TEXT AS status, payload_json, payload_bin, latest_error, updated_at
            FROM {table_name}
            WHERE id = $1
            "#
        ),
        vec![Value::Uuid(Some(id))],
    );

    let row = txn
        .query_one_raw(select_stmt)
        .await?
        .ok_or_else(|| BusError::EventNotFound(id.to_string()))?;

    let event_archive = BusEventForArchive {
        id: row.try_get("", "id")?,
        queue_name: row.try_get("", "queue_name")?,
        type_name_event: row.try_get("", "type_name_event")?,
        type_name_handler: row.try_get("", "type_name_handler")?,
        status: {
            let s: String = row.try_get("", "status")?;
            EventStatusEnum::try_from(s)?
        },

        #[cfg(feature = "json-payload")]
        payload_json: row.try_get("", "payload_json")?,

        payload_bin: row.try_get("", "payload_bin")?,
        latest_error: row.try_get("", "latest_error")?,
        created_at: row.try_get("", "updated_at")?,
    };

    let payload_json_value = {
        #[cfg(feature = "json-payload")]
        {
            Value::Json(Some(Box::new(event_archive.payload_json.to_owned())))
        }
        #[cfg(not(feature = "json-payload"))]
        {
            Value::Json(None)
        }
    };

    let insert_stmt = Statement::from_sql_and_values(
        DbBackend::Postgres,
        format!(
            r#"
            INSERT INTO {table_name_archive} (
                id, queue_name, type_name_event, type_name_handler, 
                status, payload_json, payload_bin, latest_error, created_at
            ) VALUES ($1, $2, $3, $4, $5::bus_events_status_archive_enum, $6, $7, $8, $9)
            "#
        ),
        vec![
            Value::Uuid(Some(event_archive.id.to_owned())),
            Value::String(Some(event_archive.queue_name.to_owned())),
            Value::String(Some(event_archive.type_name_event.to_owned())),
            Value::String(Some(event_archive.type_name_handler.to_owned())),
            Value::String(Some(event_archive.status.to_string())),
            payload_json_value,
            Value::Bytes(Some(event_archive.payload_bin.to_owned())),
            Value::String(event_archive.latest_error),
            Value::ChronoDateTime(Some(event_archive.created_at)),
        ],
    );

    txn.execute_raw(insert_stmt).await?;

    let delete_stmt = Statement::from_sql_and_values(
        DbBackend::Postgres,
        format!(r#"DELETE FROM {table_name} WHERE id = $1"#),
        vec![Value::Uuid(Some(id))],
    );

    txn.execute_raw(delete_stmt).await?;
    txn.commit().await.map_err(BusError::DbErr)?;
    Ok(())
}

pub(crate) async fn change_status(
    id: Uuid,
    status: EventStatusEnum,
    err: Option<Box<dyn Error + Send + Sync>>,
) -> Result<(), BusError> {
    let table_name = get_queue_config()?.connection().table_name();

    let mut sql = format!(
        r#"
        UPDATE {table_name}
        SET status = $1::bus_events_status_enum,
            updated_at = (NOW() AT TIME ZONE 'UTC')
        "#
    );

    let mut values: Vec<Value> = vec![Value::String(Some(status.to_string()))];

    if let Some(ref e) = err {
        sql.push_str(", latest_error = $2 WHERE id = $3");
        values.push(Value::String(Some(e.to_string())));
        values.push(Value::Uuid(Some(id)));
    } else {
        sql.push_str(" WHERE id = $2");
        values.push(Value::Uuid(Some(id)));
    }

    let query = Statement::from_sql_and_values(DbBackend::Postgres, sql, values);
    let db = get_db_conn()?;

    db.execute_raw(query).await?;
    Ok(())
}

#[cfg_attr(test, allow(dead_code))]
pub(crate) async fn get_queues_items(
    queue_config: &DatabaseQueueConfiguration,
) -> Result<Vec<BusEventJob>, BusError> {
    let now = Utc::now().naive_utc();
    let limit = queue_config.batch_size();
    let table_name = get_queue_config()?.connection().table_name();

    let query = Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Postgres,
        format!(
            r#"
            WITH locked_items AS (
                SELECT id
                FROM {table_name}
                WHERE queue_name = $1
                  AND status = $2::bus_events_status_enum
                  AND should_start_at <= $3
                ORDER BY should_start_at ASC
                LIMIT $4
                FOR UPDATE SKIP LOCKED
            )
            UPDATE {table_name}
            SET
              status = $5::bus_events_status_enum,
              expires_at = (NOW() AT TIME ZONE 'UTC') + expires_interval,
              updated_at = (NOW() AT TIME ZONE 'UTC')
            FROM locked_items
            WHERE {table_name}.id = locked_items.id
            RETURNING 
                {table_name}.id,
                {table_name}.queue_name,
                {table_name}.type_name_event,
                {table_name}.type_name_handler,
                {table_name}.payload_bin,
                {table_name}.retries_current,
                {table_name}.retries_max,
                {table_name}.archive_mode::TEXT,
                {table_name}.expires_at,
                EXTRACT(EPOCH FROM {table_name}.expires_interval)::BIGINT AS expires_interval
            "#
        ),
        vec![
            Value::String(Some(queue_config.queue_name().to_string())),
            Value::String(Some(EventStatusEnum::Pending.to_string())),
            Value::ChronoDateTime(Some(now)),
            Value::Int(Some(limit)),
            Value::String(Some(EventStatusEnum::Processing.to_string())),
        ],
    );

    let txn = get_db_conn()?.begin().await?;
    let rows = txn.query_all_raw(query).await?;
    txn.commit().await?;

    rows.into_iter()
        .map(|row| {
            Ok(BusEventJob {
                id: row.try_get("", "id")?,
                type_name_event: row.try_get("", "type_name_event")?,
                type_name_handler: row.try_get("", "type_name_handler")?,
                payload_bin: row.try_get("", "payload_bin")?,
                retries_current: row.try_get("", "retries_current")?,
                retries_max: row.try_get("", "retries_max")?,
                archive_mode: {
                    let s: String = row.try_get("", "archive_mode")?;
                    ArchiveType::try_from(s)?
                },
                expires_interval: {
                    let seconds: i64 = row.try_get("", "expires_interval")?;
                    std::time::Duration::from_secs(seconds as u64)
                },
            })
        })
        .collect::<Result<Vec<BusEventJob>, BusError>>()
}

pub(crate) async fn insert_to_events(
    txn: &DatabaseTransaction,
    bus_event: BusEvent,
) -> Result<(), BusError> {
    let db_config = get_queue_config()?;
    let table_name = db_config.connection().table_name();

    let payload_json_value = {
        #[cfg(feature = "json-payload")]
        {
            Value::Json(Some(Box::new(bus_event.payload_json.to_owned())))
        }
        #[cfg(not(feature = "json-payload"))]
        {
            Value::Json(None)
        }
    };

    let stmt = Statement::from_sql_and_values(
        DbBackend::Postgres,
        format!(
            r#"
            INSERT INTO {table_name} (
              id, queue_name, type_name_event, type_name_handler, status, 
              payload_json, payload_bin, retries_current, retries_max, latest_error, 
              archive_mode, should_start_at, expires_at, expires_interval, created_at, updated_at
            )
            VALUES (
              $1, $2, $3, $4, $5::bus_events_status_enum,
              $6, $7, $8, $9, $10, $11::bus_events_archive_mode_enum,
              $12, $13, $14::interval, $15, $16
            )
            "#
        ),
        vec![
            Value::Uuid(Some(bus_event.id.to_owned())),
            Value::String(Some(bus_event.queue_name.to_owned())),
            Value::String(Some(bus_event.type_name_event.to_owned())),
            Value::String(Some(bus_event.type_name_handler.to_owned())),
            Value::String(Some(bus_event.status.to_string())),
            payload_json_value,
            Value::Bytes(Some(bus_event.payload_bin.to_owned())),
            Value::Int(Some(bus_event.retries_current)),
            Value::Int(Some(bus_event.retries_max)),
            Value::String(bus_event.latest_error),
            Value::String(Some(bus_event.archive_mode.to_string())),
            Value::ChronoDateTime(Some(bus_event.should_start_at)),
            Value::ChronoDateTime(bus_event.expires_at),
            Value::String(Some(format!(
                "{} seconds",
                bus_event.expires_interval.num_seconds()
            ))),
            Value::ChronoDateTime(Some(bus_event.created_at)),
            Value::ChronoDateTime(Some(bus_event.updated_at)),
        ],
    );

    txn.execute_raw(stmt).await?;
    Ok(())
}
