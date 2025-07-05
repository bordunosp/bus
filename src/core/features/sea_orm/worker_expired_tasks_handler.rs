use crate::core::error_bus::BusError;
use crate::core::features::sea_orm::sql;
use crate::core::features::sea_orm::workers_process::process_failed;
use futures::future::join_all;
use std::error::Error;

pub(crate) async fn handle_expired_tasks() -> Result<bool, Box<dyn Error + Send + Sync>> {
    let entities = sql::get_expires().await?;

    if entities.is_empty() {
        return Ok(false);
    }

    let tasks = entities
        .iter()
        .map(|entity| process_failed(entity, Box::new(BusError::EventExpired)))
        .collect::<Vec<_>>();

    join_all(tasks).await;

    Ok(true)
}
