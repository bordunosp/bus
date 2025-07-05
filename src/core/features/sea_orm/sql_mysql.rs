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
use sea_orm::{DbErr, RuntimeErr};
use std::error::Error;
use uuid::Uuid;

pub(crate) async fn get_expires() -> Result<Vec<BusEventJob>, Box<dyn Error + Send + Sync>> {
    let query = Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::MySql,
        format!(
            r#"
            SELECT 
              id, queue_name, type_name_event, type_name_handler, payload_bin, 
              retries_current, retries_max, archive_mode, expires_at, expires_interval
            FROM {table_name}
            WHERE status = ?
              AND expires_at > ?
            ORDER BY expires_at ASC
            LIMIT 10
            "#,
            table_name = get_queue_config()?.connection().table_name().to_string()
        ),
        vec![
            Value::String(Some(Box::new(EventStatusEnum::Processing.to_string()))),
            Value::ChronoDateTime(Some(Box::new(
                Utc::now().naive_utc() + chrono::Duration::minutes(1),
            ))),
        ],
    );

    get_db_conn()?
        .query_all(query)
        .await?
        .into_iter()
        .map(|row| {
            Ok(BusEventJob {
                id: {
                    let id_bytes: Vec<u8> = row.try_get("", "id")?;
                    Uuid::from_slice(&id_bytes).map_err(|e| {
                        BusError::DbErr(DbErr::Query(RuntimeErr::Internal(e.to_string())))
                    })?
                },
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
                    let seconds: i32 = row.try_get("", "expires_interval")?;
                    std::time::Duration::from_secs(seconds as u64)
                },
            })
        })
        .collect::<Result<Vec<BusEventJob>, Box<dyn Error + Send + Sync>>>()
}

pub(crate) async fn get_id_status(
    status: EventStatusEnum,
) -> Result<Vec<IdArchiveJob>, Box<dyn Error + Send + Sync>> {
    get_db_conn()?
        .query_all(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::MySql,
            format!(
                r#"
                SELECT id, archive_mode
                FROM {table_name}
                WHERE status = ?
                LIMIT 10
                "#,
                table_name = get_queue_config()?.connection().table_name().to_string()
            ),
            vec![Value::String(Some(Box::new(status.to_string())))],
        ))
        .await?
        .into_iter()
        .map(|row| {
            Ok(IdArchiveJob {
                id: {
                    let id_bytes: Vec<u8> = row.try_get("", "id")?;
                    Uuid::from_slice(&id_bytes).map_err(|e| {
                        BusError::DbErr(DbErr::Query(RuntimeErr::Internal(e.to_string())))
                    })?
                },
                archive_mode: {
                    let s: String = row.try_get("", "archive_mode")?;
                    ArchiveType::try_from(s)?
                },
            })
        })
        .collect::<Result<Vec<IdArchiveJob>, Box<dyn Error + Send + Sync>>>()
}

pub(crate) async fn delete(id: Uuid) -> Result<(), Box<dyn Error + Send + Sync>> {
    get_db_conn()?
        .execute(Statement::from_sql_and_values(
            DbBackend::MySql,
            format!(
                r#"DELETE FROM {table_name} WHERE id = ?;"#,
                table_name = get_queue_config()?.connection().table_name().to_string()
            ),
            vec![Value::Bytes(Some(Box::new(id.as_bytes().to_vec())))],
        ))
        .await?;
    Ok(())
}

pub(crate) async fn update_scheduled_at(
    id: Uuid,
    scheduled_at: chrono::Duration,
    err: Box<dyn Error + Send + Sync>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    get_db_conn()?
        .execute(Statement::from_sql_and_values(
            DbBackend::MySql,
            format!(
                r#"
                UPDATE {table_name}
                SET status = ?,
                    retries_current = retries_current + 1,
                    latest_error = ?,
                    expires_at = ?,
                    should_start_at = ?,
                    updated_at = (NOW() AT TIME ZONE 'UTC')
                WHERE id = ?
                "#,
                table_name = get_queue_config()?.connection().table_name().to_string()
            ),
            vec![
                Value::String(Some(Box::new(EventStatusEnum::Pending.to_string()))),
                Value::String(Some(Box::new(err.to_string()))),
                Value::ChronoDateTime(None),
                Value::ChronoDateTime(Some(Box::new(Utc::now().naive_utc() + scheduled_at))),
                Value::Bytes(Some(Box::new(id.as_bytes().to_vec()))),
            ],
        ))
        .await?;

    Ok(())
}

pub(crate) async fn archive(id: Uuid) -> Result<(), BusError> {
    let db = get_db_conn()?;
    let txn = db.begin().await?;
    let table_name = get_queue_config()?.connection().table_name().to_string();
    let table_name_archive = get_queue_config()?
        .connection()
        .table_name_archive()
        .to_string();

    let select_stmt = Statement::from_sql_and_values(
        DbBackend::MySql,
        format!(
            r#"
            SELECT 
              id, queue_name, type_name_event, type_name_handler, 
              status, payload_json, payload_bin, latest_error, updated_at
            FROM {table_name}
            WHERE id = ?
            "#,
            table_name = table_name
        ),
        vec![Value::Bytes(Some(Box::new(id.as_bytes().to_vec())))],
    );

    let row = txn
        .query_one(select_stmt)
        .await?
        .ok_or_else(|| BusError::EventNotFound(id.to_string()))?;

    let event_archive = BusEventForArchive {
        id: {
            let id_bytes: Vec<u8> = row.try_get("", "id")?;
            Uuid::from_slice(&id_bytes)
                .map_err(|e| BusError::DbErr(DbErr::Query(RuntimeErr::Internal(e.to_string()))))?
        },
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
        DbBackend::MySql,
        format!(
            r#"
            INSERT INTO {table_name} (
                id, queue_name, type_name_event, type_name_handler, 
                status, payload_json, payload_bin, latest_error, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
            table_name = table_name_archive
        ),
        vec![
            Value::Bytes(Some(Box::new(event_archive.id.as_bytes().to_vec()))),
            Value::String(Some(Box::new(event_archive.queue_name.to_owned()))),
            Value::String(Some(Box::new(event_archive.type_name_event.to_owned()))),
            Value::String(Some(Box::new(event_archive.type_name_handler.to_owned()))),
            Value::String(Some(Box::new(event_archive.status.to_string()))),
            payload_json_value,
            Value::Bytes(Some(Box::new(event_archive.payload_bin.to_owned()))),
            Value::String(event_archive.latest_error.map(Box::new)),
            Value::ChronoDateTime(Some(Box::new(event_archive.created_at))),
        ],
    );

    txn.execute(insert_stmt).await?;

    let delete_stmt = Statement::from_sql_and_values(
        DbBackend::MySql,
        format!(
            r#"DELETE FROM {table_name} WHERE id = $1"#,
            table_name = table_name
        ),
        vec![Value::Bytes(Some(Box::new(id.as_bytes().to_vec())))],
    );

    txn.execute(delete_stmt).await?;
    txn.commit().await?;
    Ok(())
}

pub(crate) async fn change_status(
    id: Uuid,
    status: EventStatusEnum,
    err: Option<Box<dyn Error + Send + Sync>>,
) -> Result<(), BusError> {
    let mut sql = format!(
        r#"
        UPDATE {table_name}
        SET status = ?, 
            updated_at = (NOW() AT TIME ZONE 'UTC')
        "#,
        table_name = get_queue_config()?.connection().table_name().to_string()
    );

    let mut values: Vec<Value> = vec![Value::String(Some(Box::new(status.to_string())))];

    if let Some(ref e) = err {
        sql.push_str(", latest_error = ? WHERE id = ?");
        values.push(Value::String(Some(Box::new(e.to_string()))));
        values.push(Value::Bytes(Some(Box::new(id.as_bytes().to_vec()))));
    } else {
        sql.push_str(" WHERE id = ?");
        values.push(Value::Bytes(Some(Box::new(id.as_bytes().to_vec()))));
    }

    let query = Statement::from_sql_and_values(DbBackend::MySql, sql, values);
    let db = get_db_conn()?;

    db.execute(query).await?;
    Ok(())
}

pub(crate) async fn get_queues_items(
    queue_config: &DatabaseQueueConfiguration,
) -> Result<Vec<BusEventJob>, BusError> {
    let now = Utc::now().naive_utc();
    let limit = queue_config.batch_size();
    let table_name = get_queue_config()?.connection().table_name().to_string();
    let txn = get_db_conn()?.begin().await?;

    let select_ids_stmt = Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::MySql,
        format!(
            r#"
            SELECT id
            FROM {table_name}
            WHERE queue_name = ?
              AND status = ?
              AND should_start_at <= ?
            ORDER BY should_start_at ASC
            LIMIT ?
            FOR UPDATE SKIP LOCKED
            "#,
            table_name = table_name
        ),
        vec![
            Value::String(Some(Box::new(queue_config.queue_name().to_string()))),
            Value::String(Some(Box::new(EventStatusEnum::Pending.to_string()))),
            Value::ChronoDateTime(Some(Box::new(now))),
            Value::Int(Some(limit)),
        ],
    );

    let id_rows = txn.query_all(select_ids_stmt).await?;
    if id_rows.is_empty() {
        txn.commit().await?;
        return Ok(vec![]);
    }

    let ids: Vec<Uuid> = id_rows
        .iter()
        .map(|row| {
            let bytes: Vec<u8> = row.try_get("", "id")?;
            Uuid::from_slice(&bytes).map_err(|e| {
                BusError::DbErr(sea_orm::DbErr::Query(sea_orm::RuntimeErr::Internal(
                    e.to_string(),
                )))
            })
        })
        .collect::<Result<Vec<Uuid>, BusError>>()?;

    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
    let mut update_values: Vec<Value> = vec![Value::String(Some(Box::new(
        EventStatusEnum::Processing.to_string(),
    )))];
    for id in &ids {
        update_values.push(Value::Bytes(Some(Box::new(id.as_bytes().to_vec()))));
    }

    let update_stmt = Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::MySql,
        format!(
            r#"
            UPDATE {table_name}
            SET
              status = ?,
              expires_at = DATE_ADD(UTC_TIMESTAMP(), INTERVAL expires_interval SECOND),
              updated_at = UTC_TIMESTAMP()
            WHERE id IN ({placeholders})
            "#,
            table_name = table_name,
            placeholders = placeholders
        ),
        update_values,
    );

    txn.execute(update_stmt).await?;

    let mut select_values: Vec<Value> = vec![];
    for id in &ids {
        select_values.push(Value::Bytes(Some(Box::new(id.as_bytes().to_vec()))));
    }

    let select_stmt = Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::MySql,
        format!(
            r#"
            SELECT 
              id,
              queue_name,
              type_name_event,
              type_name_handler,
              payload_bin,
              retries_current,
              retries_max,
              archive_mode,
              expires_at,
              expires_interval
            FROM {table_name}
            WHERE id IN ({placeholders})
            "#,
            table_name = table_name,
            placeholders = placeholders
        ),
        select_values,
    );

    let rows = txn.query_all(select_stmt).await?;
    txn.commit().await?;

    rows.into_iter()
        .map(|row| {
            Ok(BusEventJob {
                id: {
                    let id_bytes: Vec<u8> = row.try_get("", "id")?;
                    Uuid::from_slice(&id_bytes).map_err(|e| {
                        BusError::DbErr(DbErr::Query(RuntimeErr::Internal(e.to_string())))
                    })?
                },
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
                    let seconds: i32 = row.try_get("", "expires_interval")?;
                    std::time::Duration::from_secs(seconds as u64)
                },
            })
        })
        .collect::<Result<Vec<BusEventJob>, _>>()
}

pub(crate) async fn insert_to_events(
    txn: &DatabaseTransaction,
    bus_event: BusEvent,
) -> Result<(), BusError> {
    let db_config = get_queue_config()?;

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
        DbBackend::MySql,
        format!(
            r#"
            INSERT INTO {table_name} (
              id, queue_name, type_name_event, type_name_handler, status, 
              payload_json, payload_bin, retries_current, retries_max, latest_error, 
              archive_mode, should_start_at, expires_at, expires_interval, created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
            table_name = db_config.connection().table_name().to_string()
        ),
        vec![
            Value::Bytes(Some(Box::new(bus_event.id.as_bytes().to_vec()))),
            Value::String(Some(Box::new(bus_event.queue_name.to_owned()))),
            Value::String(Some(Box::new(bus_event.type_name_event.to_owned()))),
            Value::String(Some(Box::new(bus_event.type_name_handler.to_owned()))),
            Value::String(Some(Box::new(bus_event.status.to_string()))),
            payload_json_value,
            Value::Bytes(Some(Box::new(bus_event.payload_bin.to_owned()))),
            Value::Int(Some(bus_event.retries_current)),
            Value::Int(Some(bus_event.retries_max)),
            Value::String(bus_event.latest_error.map(Box::new)),
            Value::String(Some(Box::new(bus_event.status.to_string()))),
            Value::String(Some(Box::new(bus_event.archive_mode.to_string()))),
            Value::ChronoDateTime(Some(Box::new(bus_event.should_start_at))),
            Value::ChronoDateTime(bus_event.expires_at.map(Box::new)),
            Value::Int(Some(bus_event.expires_interval.num_seconds() as i32)),
            Value::ChronoDateTime(Some(Box::new(bus_event.created_at))),
            Value::ChronoDateTime(Some(Box::new(bus_event.updated_at))),
        ],
    );

    txn.execute(stmt).await?;
    Ok(())
}
