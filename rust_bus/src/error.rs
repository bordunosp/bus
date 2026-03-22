use std::fmt;

#[derive(Debug)]
pub enum BusError {
    Serialization(String),
    Database(String),
    Configuration(String),
    MetaData(String),
    Context(String),
    Handler(String),
    Generic(Box<dyn std::error::Error + Send + Sync>),
}

impl std::error::Error for BusError {}

impl fmt::Display for BusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Context(msg) => write!(f, "Bus Context error: {}", msg),
            Self::Handler(msg) => write!(f, "Bus Handler error: {}", msg),
            Self::Serialization(msg) => write!(f, "Bus Serialization error: {}", msg),
            Self::Database(msg) => write!(f, "Bus Database error: {}", msg),
            Self::Configuration(msg) => write!(f, "Bus Configuration error: {}", msg),
            Self::Generic(e) => write!(f, "Bus Generic error: {}", e),
            Self::MetaData(msg) => write!(f, "Bus MetaData error: {}", msg),
        }
    }
}

#[cfg(feature = "_db_any")]
impl From<serde_json::Error> for BusError {
    fn from(e: serde_json::Error) -> Self {
        Self::Serialization(e.to_string())
    }
}

#[cfg(feature = "_db_sqlx")]
impl From<sqlx::Error> for BusError {
    fn from(e: sqlx::Error) -> Self {
        Self::Database(e.to_string())
    }
}

#[cfg(feature = "_db_sea_orm")]
impl From<sea_orm::DbErr> for BusError {
    fn from(e: sea_orm::DbErr) -> Self {
        Self::Database(e.to_string())
    }
}
