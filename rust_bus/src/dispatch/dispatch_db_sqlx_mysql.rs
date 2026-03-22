use crate::BusError;
use crate::contracts::bus_event::IBusEvent;
use crate::contracts::enums::{Field, Period, Replace, State, Timestamp, ToColumn};
use crate::contracts::meta::BusMetadata;
use crate::dispatch::dispatch_db::{INSERT_SQL, ValidatedJob};
use chrono::Utc;
use std::collections::HashMap;
use std::hash::Hasher;
use twox_hash::XxHash3_64;
use uuid::Uuid;

pub(crate) async fn insert_sqlx_mysql<TEvent>(
    txn: &mut sqlx::Transaction<'_, sqlx::MySql>,
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

    for job in jobs.iter() {
        let mut hasher = XxHash3_64::with_seed(0);
        hasher.write(job.reg.handler_identity.as_bytes());
        hasher.write(job.reg.event_identity.as_bytes());
        let hash_type_name = hasher.finish() as i64;

        let mut qb = sqlx::QueryBuilder::<sqlx::MySql>::new(
            "SELECT id, state FROM bus_jobs USE INDEX(idx_bus_jobs_hash_type_name) WHERE hash_type_name = ",
        );
        qb.push_bind(hash_type_name)
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
                qb.push(format!(" AND {} > ", time_col)).push_bind(
                    now - chrono::Duration::from_std(*duration).map_err(|_| {
                        BusError::Configuration(format!(
                            "period.duration has incorrect value for DataBase Handler '{}'",
                            &job.reg.handler_identity
                        ))
                    })?,
                );
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
                        qb.push(" AND JSON_CONTAINS(tags, ");
                        let tags_json = serde_json::to_string(&job.tags_json)
                            .unwrap_or_else(|_| "[]".to_string());
                        qb.push_bind(tags_json);
                        qb.push(")");
                    }
                    Field::Meta => {
                        qb.push(" AND JSON_CONTAINS(meta, ");
                        let meta_json_string = serde_json::to_string(&metadata_json)
                            .unwrap_or_else(|_| "{}".to_string());
                        qb.push_bind(meta_json_string);
                        qb.push(")");
                    }
                    Field::Event => {
                        qb.push(" AND payload = CAST(");
                        qb.push_bind(serde_json::to_string(&event_json)?);
                        qb.push(" AS JSON) ");
                    }
                }
            }

            for key in unique.keys.iter() {
                qb.push(" AND JSON_EXTRACT(payload, ");
                qb.push_bind(format!("$.{}", key));
                qb.push(")");

                if let Some(val) = event_json.get(*key) {
                    qb.push(" = CAST(");
                    qb.push_bind(serde_json::to_string(val)?);
                    qb.push(" AS JSON)");
                } else {
                    qb.push(" IS NULL");
                }
            }

            let states_strings: Vec<&str> = unique
                .states
                .iter()
                .map(|s| match s {
                    State::Available => "available",
                    State::Scheduled => "scheduled",
                    State::Executing => "executing",
                    State::Retryable => "retryable",
                    State::Completed => "completed",
                    State::Cancelled => "cancelled",
                    State::Discarded => "discarded",
                })
                .collect();

            if !states_strings.is_empty() {
                qb.push(" AND bus_jobs.state IN (");
                let mut separated = qb.separated(", ");
                for state in states_strings {
                    separated.push_bind(state);
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

        qb.push(" LIMIT 1 FOR UPDATE ");

        let existing: Option<(Vec<u8>, String)> = qb
            .build_query_as::<(Vec<u8>, String)>()
            .fetch_optional(&mut **txn)
            .await
            .map_err(|e| BusError::Database(e.to_string()))?;

        if let Some((existing_id, _)) = existing {
            if !update_case.is_empty() {
                let mut up_qb = sqlx::QueryBuilder::<sqlx::MySql>::new("UPDATE bus_jobs SET ");
                let mut first_col = true;

                for (col_name, states) in update_case.iter() {
                    if !first_col {
                        up_qb.push(", ");
                    }
                    first_col = false;

                    up_qb.push(col_name).push(" = CASE WHEN state IN (");
                    let mut sep = up_qb.separated(", ");
                    for state in states {
                        sep.push_bind(state);
                    }
                    up_qb.push(") THEN ");

                    match *col_name {
                        "priority" => {
                            up_qb.push_bind(job.priority);
                        }
                        "queue" => {
                            up_qb.push_bind(job.reg.queue);
                        }
                        "max_attempts" => {
                            up_qb.push_bind(job.max_attempts);
                        }
                        "execution_timeout_sec" => {
                            up_qb.push_bind(job.timeout_sec);
                        }
                        "tags" => {
                            up_qb.push_bind(serde_json::to_string(&job.tags_json)?);
                        }
                        "payload" => {
                            up_qb.push_bind(serde_json::to_string(&event_json)?);
                        }
                        "meta" => {
                            up_qb.push_bind(serde_json::to_string(&metadata_json)?);
                        }
                        _ => {
                            up_qb.push("`").push(col_name).push("` ");
                        }
                    }

                    up_qb.push(" ELSE `").push(col_name).push("` END");
                }
                up_qb.push(" WHERE id = ").push_bind(existing_id);
                up_qb
                    .build()
                    .execute(&mut **txn)
                    .await
                    .map_err(|e| BusError::Database(e.to_string()))?;
            }
        } else {
            let mut ins_qb = sqlx::QueryBuilder::<sqlx::MySql>::new(INSERT_SQL);
            ins_qb.push(" VALUES (");
            let mut sep = ins_qb.separated(", ");

            sep.push_bind(Uuid::now_v7().as_bytes().to_vec()); // id
            sep.push_bind(hash_type_name); // hash_type_name
            sep.push_bind(now); // inserted_at
            sep.push_bind(job.schedule_in); // scheduled_at

            sep.push_bind(None::<chrono::DateTime<Utc>>); // 5. attempted_at
            sep.push_bind(None::<chrono::DateTime<Utc>>); // 6. completed_at
            sep.push_bind(None::<chrono::DateTime<Utc>>); // 7. cancelled_at
            sep.push_bind(None::<chrono::DateTime<Utc>>); // 8. discarded_at

            sep.push_bind("available"); // state
            sep.push_bind(job.priority); // priority
            sep.push_bind(0i32); // attempt
            sep.push_bind(job.max_attempts); // max_attempts
            sep.push_bind(job.timeout_sec); // execution_timeout_sec
            sep.push_bind(job.reg.queue); // queue
            sep.push_bind(job.reg.event_identity); // type_name_event
            sep.push_bind(job.reg.handler_identity); // type_name_handler
            sep.push_bind(serde_json::to_string(&event_json)?); // payload
            sep.push_bind(serde_json::to_string(&metadata_json)?); // meta
            sep.push_bind(serde_json::to_string(&job.tags_json)?); // tags
            sep.push_bind("[]"); // errors
            sep.push_bind("[]"); // attempted_by
            ins_qb.push(")");

            ins_qb
                .build()
                .execute(&mut **txn)
                .await
                .map_err(|e| BusError::Database(e.to_string()))?;
        }
    }

    Ok(())
}
