use crate::{BusError, dispatch};

pub(crate) fn init_memory_handlers() -> Result<(usize, usize), BusError> {
    let mut mem_map = std::collections::HashMap::new();
    let mut mem_handlers_total = 0;

    for reg in inventory::iter::<dispatch::registration::EventHandlerRegistration>() {
        let entries = mem_map
            .entry(reg.event_identity)
            .or_insert_with(Vec::<&'static dispatch::registration::EventHandlerRegistration>::new);

        if let Some(entry) = entries.first()
            && entry.event_identity != reg.event_identity
        {
            return Err(BusError::Configuration(format!(
                "Event struct mismatch for identity '{}': handlers '{}' and '{}' use different Rust types",
                reg.event_identity, entry.handler_identity, reg.handler_identity
            )));
        }

        #[cfg(feature = "context")]
        if let Some(entry) = entries.first()
            && entry.context_identity != reg.context_identity
        {
            return Err(BusError::Configuration(format!(
                "Context mismatch for memory event '{}': handler '{}' requires {}, but handler '{}' requires {}",
                reg.event_identity,
                entry.handler_identity,
                entry.context_identity,
                reg.handler_identity,
                reg.context_identity
            )));
        }

        #[cfg(feature = "logging")]
        log::debug!(
            "Registered memory handler: {} for event: {}",
            reg.handler_identity,
            reg.event_identity
        );

        entries.push(reg);
        mem_handlers_total += 1;
    }

    let mem_events_count = mem_map.len();
    dispatch::registration::MEMORY_HANDLERS
        .set(mem_map)
        .map_err(|_| BusError::Configuration("Memory handlers already initialized".to_string()))?;

    Ok((mem_events_count, mem_handlers_total))
}
