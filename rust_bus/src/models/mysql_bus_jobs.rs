use super::sea_orm_active_enums::BusJobState;
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "bus_jobs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false, column_type = "Binary(16)")]
    pub id: Uuid,
    pub hash_type_name: i64,
    pub inserted_at: DateTimeUtc,
    pub scheduled_at: DateTimeUtc,
    pub attempted_at: Option<DateTimeUtc>,
    pub completed_at: Option<DateTimeUtc>,
    pub cancelled_at: Option<DateTimeUtc>,
    pub discarded_at: Option<DateTimeUtc>,
    pub state: BusJobState,
    pub priority: i32,
    pub attempt: i32,
    pub max_attempts: i32,
    pub execution_timeout_sec: i32,
    pub queue: String,
    #[sea_orm(column_type = "Text")]
    pub type_name_event: String,
    #[sea_orm(column_type = "Text")]
    pub type_name_handler: String,
    #[sea_orm(column_type = "JsonBinary")]
    pub payload: Json,
    #[sea_orm(column_type = "JsonBinary")]
    pub meta: Json,
    #[sea_orm(column_type = "JsonBinary")]
    pub tags: Json,
    #[sea_orm(column_type = "JsonBinary")]
    pub errors: Json,
    #[sea_orm(column_type = "JsonBinary")]
    pub attempted_by: Json,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
