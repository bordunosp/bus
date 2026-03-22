use crate::contracts::meta::BusMetadata;
use serde_json::Value;
use uuid::Uuid;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct BusJob {
    pub id: Uuid,
    pub hash_type_name: i64,
    pub attempt: i32,
    pub max_attempts: i32,
    pub execution_timeout_sec: i32,

    pub type_name_event: String,
    pub type_name_handler: String,

    pub payload: Value,
    pub meta: BusMetadata,
    pub tags: Value,
    pub errors: Value,
}
