use crate::core::contracts::IErasedEventHandlerDatabase;
use crate::core::error_bus::BusError;
use crate::core::features::sea_orm::core::ArchiveType;
use crate::core::features::sea_orm::core::DatabaseQueueConfiguration;
use crate::core::features::sea_orm::db_enums::EventStatusEnum;
use crate::core::features::sea_orm::dto::BusEventJob;
use crate::core::features::sea_orm::event_database_pipeline::{
    DatabasePipelineWrapper, get_event_database_pipelines,
};
use crate::core::features::sea_orm::event_handlers_database::{event_from_bin, handler_registry};
use crate::core::features::sea_orm::sql;
use std::error::Error;
use std::sync::Arc;
use tokio::time::timeout;

pub(crate) async fn process_batch(
    queue: &DatabaseQueueConfiguration,
) -> Result<bool, Box<dyn Error + Send + Sync>> {
    let events = sql::get_queues_items(queue).await?;

    let futures = events.iter().map(|dto| async move {
        let res = process_item(dto).await.map_err(|e| {
            #[cfg(feature = "logging")]
            log::warn!(
                "Failed to publish event {} to {}: {e}",
                dto.type_name_event.clone(),
                dto.type_name_handler.clone()
            );
            e
        });

        match res {
            Ok(_) => process_completed(dto).await,
            Err(err) => process_failed(dto, err).await,
        }
    });

    let is_empty_queue = futures.len() == 0;
    futures::future::join_all(futures).await;

    Ok(is_empty_queue)
}

async fn process_completed(dto: &BusEventJob) {
    if let Err(err) = sql::change_status(dto.id.clone(), EventStatusEnum::Completed, None).await {
        #[cfg(feature = "logging")]
        log::error!(
            "Failed to mark event as completed id: '{}' err: {}",
            dto.id.clone().to_string(),
            err
        );
    }

    match dto.archive_mode {
        ArchiveType::All | ArchiveType::Completed => {
            if let Err(err) = sql::archive(dto.id.clone()).await {
                #[cfg(feature = "logging")]
                log::error!(
                    "Failed to archive event id: '{}' err: {}",
                    dto.id.clone().to_string(),
                    err
                );
            }
        }
        ArchiveType::None | ArchiveType::Failed => {
            if let Err(err) = sql::delete(dto.id.clone()).await {
                #[cfg(feature = "logging")]
                log::error!(
                    "Failed to delete event id: '{}' err: {}",
                    dto.id.clone().to_string(),
                    err
                );
            }
        }
    }
}

pub(crate) async fn process_failed(dto: &BusEventJob, err: Box<dyn Error + Send + Sync>) {
    if (dto.retries_current + 1) >= dto.retries_max {
        if let Err(err) =
            sql::change_status(dto.id.clone(), EventStatusEnum::Failed, Some(err)).await
        {
            #[cfg(feature = "logging")]
            log::error!(
                "Failed to mark event as Failed id: '{}' err: {}",
                dto.id.clone().to_string(),
                err
            );
            return;
        }

        match dto.archive_mode {
            ArchiveType::All | ArchiveType::Failed => {
                if let Err(err) = sql::archive(dto.id.clone()).await {
                    #[cfg(feature = "logging")]
                    log::error!(
                        "Failed to archive event id: '{}' err: {}",
                        dto.id.clone().to_string(),
                        err
                    );
                }
            }
            ArchiveType::None | ArchiveType::Completed => {
                if let Err(err) = sql::delete(dto.id.clone()).await {
                    #[cfg(feature = "logging")]
                    log::error!(
                        "Failed to delete event id: '{}' err: {}",
                        dto.id.clone().to_string(),
                        err
                    );
                }
            }
        }
        return;
    }

    let handler_registration = match handler_registry().get(dto.type_name_handler.as_str()) {
        Some(handler_registration) => handler_registration,
        None => {
            #[cfg(feature = "logging")]
            log::error!("EventHandlerDatabaseNotFound: '{}'", dto.type_name_handler);
            return;
        }
    };

    let scheduled_at = (handler_registration.calculate_scheduled_at)(dto.retries_current + 1);

    if let Err(_err) = sql::update_scheduled_at(dto.id.clone(), scheduled_at, err).await {
        #[cfg(feature = "logging")]
        log::error!(
            "Failed to update last_error id: '{}' err: {}",
            dto.id.clone().to_string(),
            _err
        );
    }
}

pub(crate) async fn process_item(dto: &BusEventJob) -> Result<(), Box<dyn Error + Send + Sync>> {
    // 1. Deserialize event from binary payload
    let event_factory = event_from_bin()
        .get(dto.type_name_event.as_str())
        .ok_or_else(|| BusError::EventDeserializerNotFound(dto.type_name_event.clone()))?;

    let any_event = event_factory(&dto.payload_bin)
        .map_err(|e| BusError::DeserializationError(dto.type_name_event.clone(), e.to_string()))?;

    let handler_registration = handler_registry()
        .get(dto.type_name_handler.as_str())
        .ok_or_else(|| BusError::EventHandlerDatabaseNotFound(dto.type_name_handler.clone()))?;

    let handler = (handler_registration.factory)()
        .await
        .map_err(|_| BusError::EventHandlerDatabaseNotFound(dto.type_name_handler.clone()))?;

    let base_handler: Arc<dyn IErasedEventHandlerDatabase> = Arc::from(handler);

    let pipeline_stack =
        get_event_database_pipelines()
            .read()
            .iter()
            .rev()
            .fold(base_handler, |next, factory| {
                let pipeline = factory();
                Arc::new(DatabasePipelineWrapper { pipeline, next })
                    as Arc<dyn IErasedEventHandlerDatabase>
            });

    timeout(dto.expires_interval, pipeline_stack.handle(any_event))
        .await
        .map_err(|_| BusError::EventExpired)??;

    Ok(())
}
