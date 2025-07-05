use crate::core::features::sea_orm::core::ArchiveType;
use crate::core::features::sea_orm::db_enums::EventStatusEnum;
use chrono::NaiveDateTime;
use sea_orm::ConnectOptions;
use std::time::Duration;
use uuid::Uuid;

pub struct DatabaseQueueConfigurationDto {
    pub queue_name: String,
    pub workers: usize,
    pub batch_size: i32,
    pub sleep_interval: Duration,
    pub archive_type: ArchiveType,
}

pub struct DatabaseConnectionDto {
    pub sea_orm_connect_options: ConnectOptions,
    pub table_name: String,
    pub table_name_archive: String,
}

pub(crate) struct BusEvent {
    pub(crate) id: Uuid,
    pub(crate) queue_name: String,
    pub(crate) type_name_event: String,
    pub(crate) type_name_handler: String,
    pub(crate) status: EventStatusEnum,

    #[cfg(feature = "json-payload")]
    pub(crate) payload_json: serde_json::Value,

    pub(crate) payload_bin: Vec<u8>,
    pub(crate) retries_current: i32,
    pub(crate) retries_max: i32,
    pub(crate) latest_error: Option<String>,
    pub(crate) archive_mode: ArchiveType,
    pub(crate) should_start_at: NaiveDateTime,
    pub(crate) expires_at: Option<NaiveDateTime>,
    pub(crate) expires_interval: chrono::Duration,
    pub(crate) created_at: NaiveDateTime,
    pub(crate) updated_at: NaiveDateTime,
}

pub(crate) struct BusEventJob {
    pub(crate) id: Uuid,

    #[cfg_attr(test, allow(dead_code))]
    pub(crate) type_name_event: String,
    pub(crate) type_name_handler: String,

    #[cfg_attr(test, allow(dead_code))]
    pub(crate) payload_bin: Vec<u8>,
    pub(crate) retries_current: i32,
    pub(crate) retries_max: i32,
    pub(crate) archive_mode: ArchiveType,

    #[cfg_attr(test, allow(dead_code))]
    pub(crate) expires_interval: std::time::Duration,
}

pub(crate) struct BusEventForArchive {
    pub(crate) id: Uuid,
    pub(crate) queue_name: String,
    pub(crate) type_name_event: String,
    pub(crate) type_name_handler: String,
    pub(crate) status: EventStatusEnum,

    #[cfg(feature = "json-payload")]
    pub(crate) payload_json: serde_json::Value,

    pub(crate) payload_bin: Vec<u8>,
    pub(crate) latest_error: Option<String>,
    pub(crate) created_at: NaiveDateTime,
}

pub(crate) struct IdArchiveJob {
    pub(crate) id: Uuid,
    pub(crate) archive_mode: ArchiveType,
}
