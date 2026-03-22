use crate::BusError;
use crate::contracts::bus_event::IBusEvent;
use crate::contracts::enums::{Field, Period, Replace, Timestamp, ToColumn};
use crate::contracts::meta::BusMetadata;
use crate::dispatch::dispatch_db::ValidatedJob;
use chrono::Utc;
use std::collections::HashMap;
use std::hash::Hasher;
use twox_hash::XxHash3_64;
use uuid::Uuid;

pub(crate) async fn insert_sqlx_postgres<TEvent>(
    txn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
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
        let mut hasher = XxHash3_64::with_seed(0);
        hasher.write("bus.insert".as_bytes());
        hasher.write(job.reg.handler_identity.as_bytes());
        hasher.write(job.reg.event_identity.as_bytes());
        hasher.write(job.reg.queue.as_bytes());
        hasher.write(&event_vec);

        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(hasher.finish() as i64)
            .execute(&mut **txn)
            .await?;

        let mut hasher = XxHash3_64::with_seed(0);
        hasher.write(job.reg.handler_identity.as_bytes());
        hasher.write(job.reg.event_identity.as_bytes());
        let hash_type_name = hasher.finish() as i64;

        let mut qb = sqlx::QueryBuilder::<sqlx::Postgres>::new("WITH existing_job AS ( ");

        qb.push("SELECT id, state FROM bus_jobs WHERE hash_type_name = ")
            .push_bind(hash_type_name)
            .push(" AND type_name_handler = ")
            .push_bind(job.reg.handler_identity)
            .push(" AND type_name_event = ")
            .push_bind(job.reg.event_identity);

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

                qb.push(format!(" AND {} > ", time_col))
                    .push_bind(duration_since);
            }

            for field in unique.fields {
                match field {
                    Field::Queue => {
                        qb.push(" AND queue = ").push_bind(job.reg.queue);
                    }
                    Field::Priority => {
                        qb.push(" AND priority = ").push_bind(job.priority);
                    }
                    Field::MaxAttempts => {
                        qb.push(" AND max_attempts = ").push_bind(job.max_attempts);
                    }
                    Field::ExecutionTimeout => {
                        qb.push(" AND execution_timeout_sec = ")
                            .push_bind(job.timeout_sec);
                    }
                    Field::Tags => {
                        qb.push(" AND tags @> ")
                            .push_bind(sqlx::types::Json(&job.tags_json));
                    }
                    Field::Meta => {
                        qb.push(" AND meta @> ")
                            .push_bind(sqlx::types::Json(&metadata_json));
                    }
                    Field::Event => {
                        qb.push(" AND payload = ")
                            .push_bind(sqlx::types::Json(&event_json));
                    }
                }
            }

            for key in unique.keys.iter() {
                qb.push(" AND payload -> ").push_bind(*key);

                if let Some(val) = event_json.get(*key) {
                    qb.push(" = ");
                    qb.push_bind(sqlx::types::Json(val));
                } else {
                    qb.push(" IS NULL");
                }
            }

            if !unique.states.is_empty() {
                qb.push(" AND bus_jobs.state IN (");
                let mut first = true;
                for s in unique.states {
                    if !first {
                        qb.push(", ");
                    }
                    first = false;

                    qb.push_bind(s.to_str());
                    qb.push("::bus_job_state");
                }
                qb.push(")");
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
        qb.push(" LIMIT 1 FOR UPDATE), ");
        qb.push(" updated_job AS (");

        if update_case.is_empty() {
            qb.push("SELECT id FROM existing_job");
        } else {
            qb.push(" UPDATE bus_jobs SET ");

            for (i, (col_name, states)) in update_case.iter().enumerate() {
                if i > 0 {
                    qb.push(", ");
                }

                qb.push(col_name).push(" = CASE WHEN bus_jobs.state IN (");
                let mut first = true;
                for s in states {
                    if !first {
                        qb.push(", ");
                    }
                    first = false;

                    qb.push_bind(*s);
                    qb.push("::bus_job_state");
                }
                qb.push(") THEN ");

                match *col_name {
                    "priority" => qb.push_bind(job.priority),
                    "queue" => qb.push_bind(job.reg.queue),
                    "max_attempts" => qb.push_bind(job.max_attempts),
                    "execution_timeout_sec" => qb.push_bind(job.timeout_sec),
                    "tags" => qb.push_bind(sqlx::types::Json(job.tags_json.clone())),
                    "payload" => qb.push_bind(sqlx::types::Json(event_json.clone())),
                    "meta" => qb.push_bind(sqlx::types::Json(metadata_json.clone())),
                    _ => qb.push("bus_jobs.").push(col_name),
                };

                qb.push(" ELSE bus_jobs.").push(col_name).push(" END");
            }
            qb.push(
                " FROM existing_job WHERE bus_jobs.id = existing_job.id RETURNING bus_jobs.id ",
            );
        }
        qb.push(") ");

        // Insert
        qb.push(crate::dispatch::dispatch_db::INSERT_SQL)
            .push("SELECT ");
        let mut separated = qb.separated(", ");

        separated.push_bind(Uuid::now_v7()); // id
        separated.push_bind(hash_type_name); // hash_type_name
        separated.push_bind(now); // inserted_at
        separated.push_bind(job.schedule_in); // scheduled_at

        separated.push_bind(None::<chrono::DateTime<Utc>>); // 5. attempted_at
        separated.push_bind(None::<chrono::DateTime<Utc>>); // 6. completed_at
        separated.push_bind(None::<chrono::DateTime<Utc>>); // 7. cancelled_at
        separated.push_bind(None::<chrono::DateTime<Utc>>); // 8. discarded_at

        separated.push(" 'available'::bus_job_state "); // state
        separated.push_bind(job.priority); // priority
        separated.push_bind(0i32); // attempt
        separated.push_bind(job.max_attempts); // max_attempts
        separated.push_bind(job.timeout_sec); // execution_timeout_sec
        separated.push_bind(job.reg.queue); // queue
        separated.push_bind(job.reg.event_identity); // type_name_event
        separated.push_bind(job.reg.handler_identity); // type_name_handler
        separated.push_bind(sqlx::types::Json(&event_json)); // payload
        separated.push_bind(sqlx::types::Json(&metadata_json)); // meta
        separated.push_bind(sqlx::types::Json(&job.tags_json)); // tags
        separated.push_bind(sqlx::types::Json(serde_json::json!([]))); // errors
        separated.push_bind(sqlx::types::Json(serde_json::json!([]))); // attempted_by

        qb.push(" WHERE NOT EXISTS (SELECT 1 FROM existing_job) ");
        qb.push(" AND NOT EXISTS (SELECT 1 FROM updated_job) ");

        qb.build()
            .execute(&mut **txn)
            .await
            .map_err(|e| BusError::Database(e.to_string()))?;
    }

    Ok(())
}
