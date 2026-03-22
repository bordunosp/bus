use crate::BusError;
use crate::contracts::bus_event::IBusEvent;
use crate::contracts::enums::ToColumn;
use crate::contracts::enums::{Field, Period, Replace, Timestamp};
use crate::contracts::meta::BusMetadata;
use crate::dispatch::dispatch_db::{INSERT_SQL, ValidatedJob};
use chrono::Utc;
use sea_orm::ConnectionTrait;
use std::collections::HashMap;
use std::hash::Hasher;
use twox_hash::XxHash3_64;
use uuid::Uuid;

pub(crate) async fn insert_sea_postgres<TEvent>(
    txn: &sea_orm::DatabaseTransaction,
    jobs: Vec<ValidatedJob<'_>>,
    event: &TEvent,
    metadata: &BusMetadata,
) -> Result<(), BusError>
where
    TEvent: IBusEvent,
{
    let now = Utc::now();
    let event_json = serde_json::to_value(event)?;
    let metadata_json = serde_json::to_value(metadata)?;
    let event_vec =
        serde_json::to_vec(&event_json).map_err(|e| BusError::Serialization(e.to_string()))?;

    for job in jobs.iter() {
        let mut lock_hasher = XxHash3_64::with_seed(0);
        lock_hasher.write("bus.insert".as_bytes());
        lock_hasher.write(job.reg.handler_identity.as_bytes());
        lock_hasher.write(job.reg.event_identity.as_bytes());
        lock_hasher.write(job.reg.queue.as_bytes());
        lock_hasher.write(&event_vec);

        let stmt = sea_orm::Statement::from_sql_and_values(
            sea_orm::DbBackend::Postgres,
            "SELECT pg_advisory_xact_lock($1)",
            vec![(lock_hasher.finish() as i64).into()],
        );
        txn.execute_raw(stmt)
            .await
            .map_err(|e| BusError::Database(e.to_string()))?;

        let mut param_idx = 1;
        let mut params: Vec<sea_orm::Value> = vec![];
        let mut push_param = |val: sea_orm::Value| {
            let p = format!("${}", param_idx);
            params.push(val);
            param_idx += 1;
            p
        };

        let mut hasher = XxHash3_64::with_seed(0);
        hasher.write(job.reg.handler_identity.as_bytes());
        hasher.write(job.reg.event_identity.as_bytes());
        let hash_type_name = hasher.finish() as i64;

        let mut sql = "WITH existing_job AS ( ".to_string();

        sql.push_str(&format!(
            "SELECT id, state FROM bus_jobs WHERE hash_type_name = {} AND type_name_handler = {} AND type_name_event = {} ",
            push_param(hash_type_name.into()),
            push_param(job.reg.handler_identity.into()),
            push_param(job.reg.event_identity.into())
        ));

        let mut update_case: HashMap<&str, Vec<&str>> = HashMap::new();

        if let Some(ref unique) = job.reg.unique {
            if let Period::Duration { field, duration } = &unique.period {
                let time_col = match field {
                    Timestamp::InsertedAt => "inserted_at",
                    Timestamp::ScheduledAt => "scheduled_at",
                };

                let duration_since = now
                    - chrono::Duration::from_std(*duration).map_err(|_| {
                        BusError::Configuration(format!(
                            "period.duration has incorrect value for DataBase Handler '{}'",
                            &job.reg.handler_identity
                        ))
                    })?;

                sql.push_str(&format!(
                    " AND {} > {} ",
                    time_col,
                    push_param(duration_since.into())
                ));
            }

            for field in unique.fields {
                match field {
                    Field::Queue => sql.push_str(&format!(
                        " AND queue = {} ",
                        push_param(job.reg.queue.into())
                    )),
                    Field::Priority => sql.push_str(&format!(
                        " AND priority = {} ",
                        push_param(job.priority.into())
                    )),
                    Field::MaxAttempts => sql.push_str(&format!(
                        " AND max_attempts = {} ",
                        push_param(job.max_attempts.into())
                    )),
                    Field::ExecutionTimeout => sql.push_str(&format!(
                        " AND execution_timeout_sec = {} ",
                        push_param(job.timeout_sec.into())
                    )),
                    Field::Tags => sql.push_str(&format!(
                        " AND tags @> {} ",
                        push_param(job.tags_json.clone().into())
                    )),
                    Field::Meta => sql.push_str(&format!(
                        " AND meta @> {} ",
                        push_param(metadata_json.clone().into())
                    )),
                    Field::Event => sql.push_str(&format!(
                        " AND payload = {} ",
                        push_param(event_json.clone().into())
                    )),
                }
            }

            for key in unique.keys.iter() {
                if let Some(val) = event_json.get(*key) {
                    sql.push_str(&format!(
                        " AND payload -> {} = {} ",
                        push_param((*key).into()),
                        push_param(val.clone().into())
                    ));
                } else {
                    sql.push_str(&format!(
                        " AND payload -> {} IS NULL ",
                        push_param((*key).into())
                    ));
                }
            }

            if !unique.states.is_empty() {
                let placeholders: Vec<String> = unique
                    .states
                    .iter()
                    .map(|s| format!("{}::bus_job_state", push_param(s.to_str().into())))
                    .collect();
                sql.push_str(&format!(
                    " AND bus_jobs.state IN ({}) ",
                    placeholders.join(", ")
                ));
            }

            for replace_item in unique.replace.iter() {
                let (state, columns): (&str, Box<dyn Iterator<Item = &'static str> + '_>) =
                    match replace_item {
                        Replace::Available(f) => {
                            ("available", Box::new(f.iter().map(|f| f.to_column())))
                        }
                        Replace::Scheduled(f) => {
                            ("scheduled", Box::new(f.iter().map(|f| f.to_column())))
                        }
                        Replace::Executing(f) => {
                            ("executing", Box::new(f.iter().map(|f| f.to_column())))
                        }
                        Replace::Retryable(f) => {
                            ("retryable", Box::new(f.iter().map(|f| f.to_column())))
                        }
                        Replace::Completed(f) => {
                            ("completed", Box::new(f.iter().map(|f| f.to_column())))
                        }
                        Replace::Cancelled(f) => {
                            ("cancelled", Box::new(f.iter().map(|f| f.to_column())))
                        }
                        Replace::Discarded(f) => {
                            ("discarded", Box::new(f.iter().map(|f| f.to_column())))
                        }
                    };

                for col in columns {
                    update_case.entry(col).or_default().push(state);
                }
            }
        }
        sql.push_str(" LIMIT 1 FOR UPDATE), ");
        sql.push_str(" updated_job AS ( ");

        if update_case.is_empty() {
            sql.push_str(" SELECT id FROM existing_job ");
        } else {
            sql.push_str(" UPDATE bus_jobs SET ");

            for (i, (col_name, states)) in update_case.iter().enumerate() {
                if i > 0 {
                    sql.push_str(", ");
                }

                let state_list: Vec<String> = states
                    .iter()
                    .map(|s| format!("{}::bus_job_state", push_param((*s).into())))
                    .collect();

                sql.push_str(&format!(
                    "{} = CASE WHEN bus_jobs.state IN ({}) THEN ",
                    col_name,
                    state_list.join(", ")
                ));

                let val = match *col_name {
                    "priority" => push_param(job.priority.into()),
                    "queue" => push_param(job.reg.queue.into()),
                    "max_attempts" => push_param(job.max_attempts.into()),
                    "execution_timeout_sec" => push_param(job.timeout_sec.into()),
                    "tags" => push_param(job.tags_json.clone().into()),
                    "payload" => push_param(event_json.clone().into()),
                    "meta" => push_param(metadata_json.clone().into()),
                    _ => format!("bus_jobs.{}", col_name),
                };
                sql.push_str(&format!("{} ELSE bus_jobs.{} END", val, col_name));
            }
            sql.push_str(
                " FROM existing_job WHERE bus_jobs.id = existing_job.id RETURNING bus_jobs.id ",
            );
        }
        sql.push_str(") ");

        // Insert
        sql.push_str(INSERT_SQL);
        sql.push_str("SELECT ");

        sql.push_str(&format!("{}, ", push_param(Uuid::now_v7().into())));
        sql.push_str(&format!("{}, ", push_param(hash_type_name.into())));
        sql.push_str(&format!("{}, ", push_param(now.into())));
        sql.push_str(&format!("{}, ", push_param(job.schedule_in.into())));

        sql.push_str("NULL, "); // attempted_at
        sql.push_str("NULL, "); // completed_at
        sql.push_str("NULL, "); // cancelled_at
        sql.push_str("NULL, "); // discarded_at

        sql.push_str("'available'::bus_job_state, ");
        sql.push_str(&format!("{}, ", push_param(job.priority.into())));
        sql.push_str("0, ");
        sql.push_str(&format!("{}, ", push_param(job.max_attempts.into())));
        sql.push_str(&format!("{}, ", push_param(job.timeout_sec.into())));
        sql.push_str(&format!("{}, ", push_param(job.reg.queue.into())));
        sql.push_str(&format!("{}, ", push_param(job.reg.event_identity.into())));
        sql.push_str(&format!(
            "{}, ",
            push_param(job.reg.handler_identity.into())
        ));
        sql.push_str(&format!("{}, ", push_param(event_json.clone().into())));
        sql.push_str(&format!("{}, ", push_param(metadata_json.clone().into())));
        sql.push_str(&format!("{}, ", push_param(job.tags_json.clone().into())));
        sql.push_str(&format!("{}, ", push_param(serde_json::json!([]).into())));
        sql.push_str(&format!("{} ", push_param(serde_json::json!([]).into())));

        sql.push_str(" WHERE NOT EXISTS (SELECT 1 FROM existing_job) ");
        sql.push_str(" AND NOT EXISTS (SELECT 1 FROM updated_job)");

        txn.execute_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DbBackend::Postgres,
            sql,
            params,
        ))
        .await
        .map_err(|e| BusError::Database(e.to_string()))?;
    }

    Ok(())
}
