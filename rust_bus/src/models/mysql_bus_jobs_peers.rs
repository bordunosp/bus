use sea_orm::entity::prelude::*;

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "bus_jobs_peers")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub name: String,

    #[sea_orm(column_type = "Binary(16)")]
    pub node: Uuid,
    pub started_at: DateTimeUtc,
    pub expires_at: DateTimeUtc,
}

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
