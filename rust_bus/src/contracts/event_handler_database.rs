use crate::contracts::bus_event::IBusEvent;
use crate::contracts::database_unique::Unique;
use crate::contracts::enums::ScheduleIn;

pub trait IEventHandlerDatabase<TEvent>: Default + Send + Sync + 'static
where
    TEvent: IBusEvent,
{
    const QUEUE: &'static str;
    const PRIORITY: u32 = 0;
    const MAX_ATTEMPTS: Option<u32> = None;
    const EXECUTION_TIMEOUT: Option<chrono::Duration> = None;
    const SCHEDULE_IN: ScheduleIn = ScheduleIn::Immediately;
    const TAGS: &'static [&'static str] = &[];
    const UNIQUE: Option<Unique> = None;

    fn next_attempt_at(current_attempt: u32) -> chrono::Duration {
        let attempt = current_attempt.max(1);
        let multiplier = 2_i64.saturating_pow(attempt - 1);
        let minutes = multiplier.min(60);
        chrono::Duration::try_minutes(minutes).unwrap_or_else(|| chrono::Duration::minutes(60))
    }

    #[cfg(feature = "_db_sea_orm")]
    fn handle(
        &self,
        db: &sea_orm::DatabaseConnection,
        event: &TEvent,
        metadata: &crate::contracts::meta::BusMetadata,
    ) -> impl Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send;

    #[cfg(feature = "sqlx-postgres")]
    fn handle(
        &self,
        db: &sqlx::PgPool,
        event: &TEvent,
        metadata: &crate::contracts::meta::BusMetadata,
    ) -> impl Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send;

    #[cfg(feature = "sqlx-mysql")]
    fn handle(
        &self,
        db: &sqlx::MySqlPool,
        event: &TEvent,
        metadata: &crate::contracts::meta::BusMetadata,
    ) -> impl Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send;
}
