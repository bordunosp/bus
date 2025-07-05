use crate::core::features::sea_orm::core::ArchiveType;
use crate::core::features::sea_orm::db_enums::EventStatusEnum;
use crate::core::features::sea_orm::sql;
use futures::future::join_all;
use std::error::Error;
use std::pin::Pin;

pub(crate) async fn handle_failed_tasks() -> Result<bool, Box<dyn Error + Send + Sync>> {
    let entities = sql::get_id_status(EventStatusEnum::Failed).await?;

    if entities.is_empty() {
        return Ok(false);
    }

    let tasks = entities
        .into_iter()
        .map(|entity| {
            let fut: Pin<
                Box<dyn Future<Output = Result<(), Box<dyn Error + Send + Sync>>> + Send>,
            > = match entity.archive_mode {
                ArchiveType::All | ArchiveType::Failed => {
                    Box::pin(async move { sql::archive(entity.id).await.map_err(|e| e.into()) })
                }
                ArchiveType::None | ArchiveType::Completed => {
                    Box::pin(async move { sql::delete(entity.id).await.map_err(|e| e.into()) })
                }
            };
            fut
        })
        .collect::<Vec<_>>();

    for result in join_all(tasks).await {
        result?;
    }

    Ok(true)
}
