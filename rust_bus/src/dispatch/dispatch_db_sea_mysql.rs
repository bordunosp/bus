use crate::BusError;
use crate::contracts::bus_event::IBusEvent;
use crate::contracts::enums::{Field, Period, Replace, State, Timestamp, ToColumn};
use crate::contracts::meta::BusMetadata;
use crate::dispatch::dispatch_db::ValidatedJob;
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ConnectionTrait};
use std::collections::HashMap;
use std::hash::Hasher;
use twox_hash::XxHash3_64;
use uuid::Uuid;

pub(crate) async fn insert_sea_mysql<TEvent>(
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

    for job in jobs.iter() {
        let mut hasher = XxHash3_64::with_seed(0);
        hasher.write(job.reg.handler_identity.as_bytes());
        hasher.write(job.reg.event_identity.as_bytes());
        let hash_type_name = hasher.finish() as i64;

        let mut sql =
            "SELECT id, state FROM bus_jobs USE INDEX(idx_bus_jobs_hash_type_name) WHERE hash_type_name = ? AND type_name_handler = ? AND type_name_event = ? "
                .to_string();

        let mut params: Vec<sea_orm::Value> = vec![
            hash_type_name.into(),
            job.reg.handler_identity.into(),
            job.reg.event_identity.into(),
        ];

        let mut update_case: HashMap<&str, Vec<&str>> = HashMap::new();

        if let Some(ref unique) = job.reg.unique {
            if let Period::Duration { field, duration } = &unique.period {
                match field {
                    Timestamp::InsertedAt => sql.push_str(" AND inserted_at > ? "),
                    Timestamp::ScheduledAt => sql.push_str(" AND scheduled_at > ? "),
                };

                let duration_since = now
                    - chrono::Duration::from_std(*duration).map_err(|_| {
                        BusError::Configuration(format!(
                            "period.duration has incorrect value for DataBase Handler '{}'",
                            &job.reg.handler_identity
                        ))
                    })?;

                params.push(duration_since.into());
            }

            for field in unique.fields {
                match field {
                    Field::Queue => {
                        sql.push_str(" AND queue = ? ");
                        params.push(job.reg.queue.into());
                    }
                    Field::Priority => {
                        sql.push_str(" AND priority = ? ");
                        params.push(job.priority.into());
                    }
                    Field::MaxAttempts => {
                        sql.push_str(" AND max_attempts = ? ");
                        params.push(job.max_attempts.into());
                    }
                    Field::ExecutionTimeout => {
                        sql.push_str(" AND execution_timeout_sec = ? ");
                        params.push(job.timeout_sec.into());
                    }
                    Field::Tags => {
                        let tags_json = serde_json::to_string(&job.tags_json)
                            .unwrap_or_else(|_| "[]".to_string());

                        sql.push_str(" AND JSON_CONTAINS(tags, ?) ");
                        params.push(tags_json.into());
                    }
                    Field::Meta => {
                        let meta_json_string = serde_json::to_string(&metadata_json)
                            .unwrap_or_else(|_| "{}".to_string());

                        sql.push_str(" AND JSON_CONTAINS(meta, ?) ");
                        params.push(meta_json_string.into());
                    }
                    Field::Event => {
                        sql.push_str(" AND payload = CAST(? AS JSON) ");
                        let event_string = serde_json::to_string(&event_json)?;
                        params.push(event_string.into());
                    }
                }
            }

            for key in unique.keys.iter() {
                let path = format!("$.\"{}\"", key.replace('\\', "\\\\").replace('"', "\\\""));

                if let Some(val) = event_json.get(*key) {
                    match val {
                        serde_json::Value::Null => {
                            sql.push_str(" AND JSON_EXTRACT(payload, ?) IS NULL ");
                            params.push(path.into());
                        }
                        serde_json::Value::String(s) => {
                            sql.push_str(" AND payload->>? = ? ");
                            params.push(path.into());
                            params.push(s.clone().into());
                        }
                        serde_json::Value::Number(n) => {
                            sql.push_str(" AND payload->>? = ? ");
                            params.push(path.into());
                            params.push(n.to_string().into());
                        }
                        serde_json::Value::Bool(b) => {
                            sql.push_str(" AND payload->>? = ? ");
                            params.push(path.into());
                            params.push(if *b { "true" } else { "false" }.into());
                        }
                        _ => {
                            sql.push_str(" AND JSON_EXTRACT(payload, ?) = CAST(? AS JSON) ");
                            params.push(path.into());
                            params.push(serde_json::to_string(val)?.into());
                        }
                    }
                } else {
                    sql.push_str(" AND JSON_CONTAINS_PATH(payload, 'one', ?) = 0 ");
                    params.push(path.into());
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
                let placeholders: String = states_strings
                    .iter()
                    .map(|_| "?")
                    .collect::<Vec<_>>()
                    .join(", ");

                sql.push_str(" AND bus_jobs.state IN (");
                sql.push_str(&placeholders);
                sql.push_str(") ");

                for state in states_strings {
                    params.push(state.into());
                }
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

        sql.push_str(" LIMIT 1 FOR UPDATE ");

        let stmt = sea_orm::Statement::from_sql_and_values(sea_orm::DbBackend::MySql, sql, params);

        let query_res = txn
            .query_one_raw(stmt)
            .await
            .map_err(|e| BusError::Database(e.to_string()))?;

        let existing = if let Some(row) = query_res {
            let id: Vec<u8> = row
                .try_get("", "id")
                .map_err(|e| BusError::Database(e.to_string()))?;

            let state: String = row
                .try_get("", "state")
                .map_err(|e| BusError::Database(e.to_string()))?;

            Some((id, state))
        } else {
            None
        };

        if let Some((existing_id, _)) = existing {
            if !update_case.is_empty() {
                let mut sql_up = "UPDATE bus_jobs SET ".to_string();
                let mut params_up: Vec<sea_orm::Value> = vec![];

                let mut first_col = true;

                for (col_name, states) in update_case.iter() {
                    if !first_col {
                        sql_up.push_str(", ");
                    }
                    first_col = false;

                    let states_placeholders: String =
                        states.iter().map(|_| "?").collect::<Vec<_>>().join(", ");

                    sql_up.push_str(col_name);
                    sql_up.push_str(" = CASE WHEN state IN (");
                    sql_up.push_str(&states_placeholders);
                    sql_up.push_str(") THEN ");

                    for state in states {
                        params_up.push((*state).into());
                    }

                    match *col_name {
                        "priority" => {
                            sql_up.push_str(" ? ");
                            params_up.push(job.priority.into());
                        }
                        "queue" => {
                            sql_up.push_str(" ? ");
                            params_up.push(job.reg.queue.into());
                        }
                        "max_attempts" => {
                            sql_up.push_str(" ? ");
                            params_up.push(job.max_attempts.into());
                        }
                        "execution_timeout_sec" => {
                            sql_up.push_str(" ? ");
                            params_up.push(job.timeout_sec.into());
                        }
                        "tags" => {
                            sql_up.push_str(" ? ");
                            params_up.push(serde_json::to_string(&job.tags_json)?.into());
                        }
                        "payload" => {
                            sql_up.push_str(" ? ");
                            params_up.push(serde_json::to_string(&event_json)?.into());
                        }
                        "meta" => {
                            sql_up.push_str(" ? ");
                            params_up.push(serde_json::to_string(&metadata_json)?.into());
                        }
                        _ => {
                            sql_up.push_str("`");
                            sql_up.push_str(col_name);
                            sql_up.push_str("` ");
                        }
                    }

                    sql_up.push_str(" ELSE `");
                    sql_up.push_str(col_name);
                    sql_up.push_str("` END");
                }
                sql_up.push_str(" WHERE id = ? ");
                params_up.push(existing_id.into());

                let stmt_up = sea_orm::Statement::from_sql_and_values(
                    sea_orm::DbBackend::MySql,
                    sql_up,
                    params_up,
                );

                txn.execute_raw(stmt_up)
                    .await
                    .map_err(|e| sea_orm::DbErr::Custom(e.to_string()))?;
            }
        } else {
            let active_model = crate::models::bus_jobs::ActiveModel {
                id: sea_orm::Set(Uuid::now_v7()),
                hash_type_name: sea_orm::Set(hash_type_name),
                inserted_at: sea_orm::Set(now),
                scheduled_at: sea_orm::Set(job.schedule_in),
                attempted_at: sea_orm::Set(None),
                completed_at: sea_orm::Set(None),
                cancelled_at: sea_orm::Set(None),
                discarded_at: sea_orm::Set(None),
                state: sea_orm::Set(crate::models::sea_orm_active_enums::BusJobState::Available),
                priority: sea_orm::Set(job.priority),
                attempt: sea_orm::Set(0i32),
                max_attempts: sea_orm::Set(job.max_attempts),
                execution_timeout_sec: sea_orm::Set(job.timeout_sec),
                queue: sea_orm::Set(job.reg.queue.to_owned()),
                type_name_event: sea_orm::Set(job.reg.event_identity.to_owned()),
                type_name_handler: sea_orm::Set(job.reg.handler_identity.to_owned()),
                payload: sea_orm::Set(event_json.to_owned()),
                meta: sea_orm::Set(metadata_json.to_owned()),
                tags: sea_orm::Set(job.tags_json.to_owned()),
                errors: sea_orm::Set(serde_json::json!([])),
                attempted_by: sea_orm::Set(serde_json::json!([])),
            };

            active_model
                .insert(txn)
                .await
                .map_err(|e| BusError::Database(e.to_string()))?;
        }
    }

    Ok(())
}
