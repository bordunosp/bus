use sea_orm::entity::prelude::*;

#[derive(Debug, Clone, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "Text", enum_name = "bus_job_state")]
pub enum BusJobState {
    #[sea_orm(string_value = "available")]
    Available,
    #[sea_orm(string_value = "scheduled")]
    Scheduled,
    #[sea_orm(string_value = "executing")]
    Executing,
    #[sea_orm(string_value = "retryable")]
    Retryable,
    #[sea_orm(string_value = "completed")]
    Completed,
    #[sea_orm(string_value = "cancelled")]
    Cancelled,
    #[sea_orm(string_value = "discarded")]
    Discarded,
}
