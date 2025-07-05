use crate::core::error_bus::BusError;
use sea_orm::DbErr;
use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum EventStatusEnum {
    Pending,
    Processing,
    Failed,
    Completed,
}

impl fmt::Display for EventStatusEnum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EventStatusEnum::Pending => write!(f, "pending"),
            EventStatusEnum::Processing => write!(f, "processing"),
            EventStatusEnum::Failed => write!(f, "failed"),
            EventStatusEnum::Completed => write!(f, "completed"),
        }
    }
}

impl TryFrom<&str> for EventStatusEnum {
    type Error = BusError;

    fn try_from(value: &str) -> Result<Self, BusError> {
        match value.trim().to_lowercase().as_str() {
            "pending" => Ok(EventStatusEnum::Pending),
            "processing" => Ok(EventStatusEnum::Processing),
            "failed" => Ok(EventStatusEnum::Failed),
            "completed" => Ok(EventStatusEnum::Completed),
            _ => Err(BusError::DbErr(DbErr::Custom(
                "Unknown EventStatusEnum type".into(),
            ))),
        }
    }
}

impl TryFrom<String> for EventStatusEnum {
    type Error = BusError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_from(value.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::string::ToString;

    #[test]
    fn test_display_event_status_enum() {
        assert_eq!(EventStatusEnum::Pending.to_string(), "pending");
        assert_eq!(EventStatusEnum::Processing.to_string(), "processing");
        assert_eq!(EventStatusEnum::Failed.to_string(), "failed");
        assert_eq!(EventStatusEnum::Completed.to_string(), "completed");
    }

    #[test]
    fn test_try_from_valid_strings() {
        assert_eq!(
            EventStatusEnum::try_from("pending").unwrap(),
            EventStatusEnum::Pending
        );
        assert_eq!(
            EventStatusEnum::try_from("PROCESSING").unwrap(),
            EventStatusEnum::Processing
        );
        assert_eq!(
            EventStatusEnum::try_from(" Failed ").unwrap(),
            EventStatusEnum::Failed
        );
        assert_eq!(
            EventStatusEnum::try_from("completed").unwrap(),
            EventStatusEnum::Completed
        );
    }

    #[test]
    fn test_try_from_invalid_string() {
        let err = EventStatusEnum::try_from("unknown").unwrap_err();
        match err {
            BusError::DbErr(db_err) => {
                let msg = db_err.to_string();
                assert!(msg.contains("Unknown EventStatusEnum type"));
            }
            _ => panic!("Expected BusError::DbErr"),
        }
    }
}
