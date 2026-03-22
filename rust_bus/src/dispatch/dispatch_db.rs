use crate::contracts::bus_event::IBusEvent;
use crate::contracts::enums::{Replace, ScheduleIn};
use crate::contracts::meta::BusMetadata;
use crate::dispatch::registration::{DATABASE_HANDLERS, EventDatabaseHandlerRegistration};
use crate::error::BusError;
use crate::workers::configuration::BusQueueConfiguration;
use chrono::{DateTime, Utc};

#[cfg(feature = "sea-orm-mysql")]
use crate::dispatch::dispatch_db_sea_mysql::insert_sea_mysql;
#[cfg(feature = "sea-orm-postgres")]
use crate::dispatch::dispatch_db_sea_postgres::insert_sea_postgres;
#[cfg(feature = "sqlx-mysql")]
use crate::dispatch::dispatch_db_sqlx_mysql::insert_sqlx_mysql;
#[cfg(feature = "sqlx-postgres")]
use crate::dispatch::dispatch_db_sqlx_postgres::insert_sqlx_postgres;

#[cfg(not(feature = "sea-orm-mysql"))]
pub(crate) const INSERT_SQL: &str = r#"
INSERT INTO bus_jobs (
    id,
    hash_type_name,

    inserted_at,
    scheduled_at,
    attempted_at,
    completed_at,
    cancelled_at,
    discarded_at,

    state,
    priority,
    attempt,
    max_attempts,
    execution_timeout_sec,

    queue,
    type_name_event,
    type_name_handler,

    payload,
    meta,
    tags,
    errors,
    attempted_by
) "#;

pub(crate) async fn dispatch_db<'a, TEvent>(
    #[cfg(feature = "sqlx-postgres")] txn: &mut sqlx::Transaction<'a, sqlx::Postgres>,
    #[cfg(feature = "sqlx-mysql")] txn: &mut sqlx::Transaction<'a, sqlx::MySql>,
    #[cfg(feature = "_db_sea_orm")] txn: &'a sea_orm::DatabaseTransaction,
    event: &TEvent,
    metadata: &BusMetadata,
) -> Result<usize, BusError>
where
    TEvent: IBusEvent,
{
    let event_identity = TEvent::EVENT_IDENTITY;

    #[cfg(feature = "logging")]
    log::debug!("Dispatching database event: {}", event_identity);

    let handlers_map = DATABASE_HANDLERS.get().ok_or_else(|| {
        BusError::Configuration(
            "Bus not initialized. Call Bus::init() before dispatching.".to_string(),
        )
    })?;

    let handlers = match handlers_map.get(event_identity) {
        None => return Ok(0),
        Some(h) if h.is_empty() => return Ok(0),
        Some(h) => h,
    };

    let global_config = BusQueueConfiguration::global()?;
    let jobs = get_validated_handlers(handlers, global_config)?;

    #[cfg(feature = "sea-orm-postgres")]
    insert_sea_postgres(txn, jobs, event, metadata).await?;

    #[cfg(feature = "sea-orm-mysql")]
    insert_sea_mysql(txn, jobs, event, metadata).await?;

    #[cfg(feature = "sqlx-postgres")]
    insert_sqlx_postgres(txn, jobs, event, metadata).await?;

    #[cfg(feature = "sqlx-mysql")]
    insert_sqlx_mysql(txn, jobs, event, metadata).await?;

    Ok(handlers.len())
}

pub(crate) struct ValidatedJob<'a> {
    pub(crate) reg: &'a EventDatabaseHandlerRegistration,
    pub(crate) max_attempts: i32,
    pub(crate) timeout_sec: i32,
    pub(crate) priority: i32,
    pub(crate) tags_json: serde_json::Value,
    pub(crate) schedule_in: DateTime<Utc>,
}

pub(crate) fn get_validated_handlers<'a>(
    handlers: &Vec<&'a EventDatabaseHandlerRegistration>,
    global_config: &'a BusQueueConfiguration,
) -> Result<Vec<ValidatedJob<'a>>, BusError> {
    let now = Utc::now();

    handlers
        .iter()
        .map(|reg| {
            let q_conf = global_config.get_queue(reg.queue)?;

            let max_attempts: i32 = reg
                .max_attempts
                .unwrap_or(q_conf.max_attempts)
                .try_into()
                .map_err(|_| BusError::Configuration("Attempts too large".into()))?;

            let timeout_sec: i32 = reg
                .execution_timeout
                .unwrap_or(q_conf.execution_timeout)
                .num_seconds()
                .try_into()
                .map_err(|_| BusError::Configuration("Timeout too large".into()))?;

            let priority: i32 = reg
                .priority
                .try_into()
                .map_err(|_| BusError::Configuration("priority too large".into()))?;

            let tags_json = serde_json::to_value(reg.tags)
                .map_err(|_| BusError::Configuration("Tags has incorrect value".into()))?;

            let schedule_in = match reg.schedule_in {
                ScheduleIn::Immediately => now,
                ScheduleIn::Duration(duration) => {
                    Utc::now()
                        + chrono::TimeDelta::from_std(duration).map_err(|_| {
                            BusError::Configuration("schedule_in has incorrect value".into())
                        })?
                }
            };

            if let Some(ref unique) = reg.unique {
                let mut seen = std::collections::HashSet::new();
                for r in unique.replace.iter() {
                    let state_id = match r {
                        Replace::Available(_) => "available",
                        Replace::Scheduled(_) => "scheduled",
                        Replace::Executing(_) => "executing",
                        Replace::Retryable(_) => "retryable",
                        Replace::Completed(_) => "completed",
                        Replace::Cancelled(_) => "cancelled",
                        Replace::Discarded(_) => "discarded",
                    };
                    if !seen.insert(state_id) {
                        return Err(BusError::Configuration(format!(
                            "replace has non unique value '{}'",
                            state_id
                        )));
                    }
                }
            }

            Ok(ValidatedJob {
                reg,
                max_attempts,
                timeout_sec,
                priority,
                tags_json,
                schedule_in,
            })
        })
        .collect()
}
