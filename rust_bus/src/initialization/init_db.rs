use crate::workers::configuration::BusQueueConfiguration;
use crate::{BusError, dispatch, workers};
use std::hash::Hasher;
use twox_hash::XxHash3_64;

pub(crate) async fn init_database_handlers(
    queue_configuration: workers::configuration::BusQueueConfigurationBuilder,
) -> Result<(usize, usize), BusError> {
    let mut db_map = std::collections::HashMap::new();
    let mut db_hash_map = std::collections::HashMap::new();
    let mut db_handlers_total = 0;

    for reg in inventory::iter::<dispatch::registration::EventDatabaseHandlerRegistration>() {
        if !queue_configuration.queues.contains_key(reg.queue) {
            return Err(BusError::Configuration(format!(
                "Handler '{}' points to queue '{}', but it is not configured. Add .add_queue(\"{}\", ...) to builder.",
                reg.handler_identity, reg.queue, reg.queue
            )));
        }

        let entries = db_map.entry(reg.event_identity).or_insert_with(
            Vec::<&'static dispatch::registration::EventDatabaseHandlerRegistration>::new,
        );

        if let Some(entry) = entries.first()
            && entry.event_identity != reg.event_identity
        {
            return Err(BusError::Configuration(format!(
                "Event struct mismatch for DB identity '{}':\n  - handler '{}' uses type '{}'\n  - handler '{}' uses type '{}'",
                reg.event_identity,
                entry.handler_identity,
                entry.event_identity,
                reg.handler_identity,
                reg.event_identity
            )));
        }

        #[cfg(feature = "logging")]
        log::debug!(
            "Registered DB handler: {} [Queue: {}] for event: {}",
            reg.handler_identity,
            reg.queue,
            reg.event_identity
        );

        entries.push(reg);
        db_handlers_total += 1;

        let mut hasher = XxHash3_64::with_seed(0);
        hasher.write(reg.handler_identity.as_bytes());
        hasher.write(reg.event_identity.as_bytes());
        let hash_type_name = hasher.finish() as i64;

        if db_hash_map.insert(hash_type_name, reg).is_some() {
            return Err(BusError::Configuration(format!(
                "Hash collision detected for handler '{}'. This is extremely rare, but you might need to rename it.",
                reg.handler_identity
            )));
        }
    }

    let db_events_count = db_map.len();
    dispatch::registration::DATABASE_HANDLERS
        .set(db_map)
        .map_err(|_| BusError::Configuration("DB handlers already initialized".to_string()))?;

    dispatch::registration::DATABASE_HANDLERS_BY_HASH
        .set(db_hash_map)
        .map_err(|_| BusError::Configuration("DB handlers already initialized".to_string()))?;

    let final_config = queue_configuration.build().await?;
    BusQueueConfiguration::set_global(final_config).map_err(|_| {
        BusError::Configuration("BusQueueConfiguration already initialized".to_string())
    })?;

    Ok((db_events_count, db_handlers_total))
}
