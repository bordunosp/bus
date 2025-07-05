use thiserror::Error;

#[cfg(any(feature = "sea-orm-postgres", feature = "sea-orm-mysql"))]
use sea_orm::DbErr;

#[derive(Debug, Error, PartialEq)]
pub enum BusError {
    #[error("BusError: CancellationToken called")]
    CancellationToken,

    #[error("BusError: EventExpired")]
    EventExpired,

    #[error(
        "BusError: `{0}` has already been initialized. Initialization must only be called once. \
If this is unexpected, check for duplicate calls to `bus::initialization::sea_orm()`."
    )]
    AlreadyInitialized(String),

    #[error(
        "BusError: `{0}` has not been initialized. You must call `bus::initialization::sea_orm()` before using this component."
    )]
    NotInitialized(String),

    #[error("BusError: Cant Serialize event '{0}' error '{1}'")]
    SerializationError(String, String),

    #[error("BusError: DeserializationError: '{0}' error '{1}'")]
    DeserializationError(String, String),

    #[error("BusError: Incorrect Type after Deserialization: '{0}'")]
    EventDeserializerTypeMismatch(String),

    #[error("BusError: Cant invocation Request Factory for request '{0}' error '{1}'")]
    RequestHandlerFactoryFailed(String, String),

    #[error("BusError: Incorrect Request type '{0}' by handler '{1}'")]
    RequestIncorrectRequestType(String, String),

    #[error("BusError: Incorrect RequestPipeline type '{0}' by handler '{1}'")]
    RequestPipelineIncorrectRequestType(String, String),

    #[error("BusError: Incorrect ResponsePipeline type '{0}' by handler '{1}'")]
    RequestPipelineIncorrectResponseType(String, String),

    #[error(
        "BusError: RequestPipeline handler registered already. Request type '{0}' by handler '{1}'"
    )]
    RequestPipelineAlreadyRegistered(String, String),

    #[error("BusError: Request handler registered already. Request type '{0}' by handler '{1}'")]
    RequestHandlerRegistered(String, String),

    #[error("BusError: EventHandlerDatabase registered already: '{0}'")]
    EventHandlerDatabaseRegistered(String),

    #[error("BusError: EventHandlerDatabase not found: '{0}'")]
    EventHandlerDatabaseNotFound(String),

    #[error("BusError: EventDeserializerNotFound not found: '{0}'")]
    EventDeserializerNotFound(String),

    #[error("BusError: No request handler found for request type '{0}'")]
    RequestHandlerNotFound(String),

    #[error("BusError: EventNotFound by id '{0}'")]
    EventNotFound(String),

    #[error("BusError: Not Found queue config by name '{0}'")]
    EventQueueSettingsNotFoundConfig(String),

    #[error(
        "BusError: Request handler return incorrect response type for request type '{0}' and response type: '{1}'"
    )]
    RequestHandlerIncorrectResponseType(String, String),

    #[error(
        "BusError: Request handler return incorrect error type for request type '{0}' and response type: '{1}'"
    )]
    RequestHandlerIncorrectErrorType(String, String),

    #[error("BusError: Incorrect Event type '{0}' by handler '{1}'")]
    EventIncorrectRequestType(String, String),

    #[error("BusError: Incorrect Error type '{0}' by event '{1}'. Error: '{2}'")]
    EventIncorrectErrorType(String, String, String),

    #[error("BusError: No event handler found for event type '{0}'")]
    EventHandlerNotFound(String),

    #[error("BusError: Incorrect Event type '{0}' by handler '{1}'")]
    EventDatabaseIncorrectRequestType(String, String),

    #[error("BusError: No event handler found for event type '{0}'")]
    EventDatabaseHandlerNotFound(String),

    #[cfg(any(feature = "sea-orm-postgres", feature = "sea-orm-mysql"))]
    #[error("BusError: DbErr: {0}")]
    DbErr(#[from] DbErr),

    #[cfg(any(feature = "sea-orm-postgres", feature = "sea-orm-mysql"))]
    #[error("BusError: Unsupported database name: '{0}'")]
    InvalidTableName(String),

    #[cfg(any(feature = "sea-orm-postgres", feature = "sea-orm-mysql"))]
    #[error("BusError: Invalid Database Queue Configuration: '{0}'")]
    InvalidDatabaseQueueConfiguration(String),

    #[cfg(any(feature = "sea-orm-postgres", feature = "sea-orm-mysql"))]
    #[error("BusError: Unsupported database backend: {0}")]
    UnsupportedDatabaseBackend(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_all_core_variants() {
        let cases: Vec<(BusError, &str)> = vec![
            (
                BusError::CancellationToken,
                "BusError: CancellationToken called",
            ),
            (
                BusError::AlreadyInitialized("MyComponent".into()),
                "BusError: `MyComponent` has already been initialized. Initialization must only be called once. \
If this is unexpected, check for duplicate calls to `bus::initialization::sea_orm()`.",
            ),
            (
                BusError::NotInitialized("MyComponent".into()),
                "BusError: `MyComponent` has not been initialized. You must call `bus::initialization::sea_orm()` before using this component.",
            ),
            (
                BusError::SerializationError("MyEvent".into(), "oops".into()),
                "BusError: Cant Serialize event 'MyEvent' error 'oops'",
            ),
            (
                BusError::DeserializationError("MyEvent".into(), "bad format".into()),
                "BusError: DeserializationError: 'MyEvent' error 'bad format'",
            ),
            (
                BusError::EventDeserializerTypeMismatch("MyEvent".into()),
                "BusError: Incorrect Type after Deserialization: 'MyEvent'",
            ),
            (
                BusError::RequestHandlerFactoryFailed("MyRequest".into(), "boom".into()),
                "BusError: Cant invocation Request Factory for request 'MyRequest' error 'boom'",
            ),
            (
                BusError::RequestIncorrectRequestType("MyRequest".into(), "MyHandler".into()),
                "BusError: Incorrect Request type 'MyRequest' by handler 'MyHandler'",
            ),
            (
                BusError::RequestPipelineIncorrectRequestType(
                    "MyRequest".into(),
                    "MyHandler".into(),
                ),
                "BusError: Incorrect RequestPipeline type 'MyRequest' by handler 'MyHandler'",
            ),
            (
                BusError::RequestPipelineIncorrectResponseType(
                    "MyRequest".into(),
                    "MyHandler".into(),
                ),
                "BusError: Incorrect ResponsePipeline type 'MyRequest' by handler 'MyHandler'",
            ),
            (
                BusError::RequestPipelineAlreadyRegistered("MyRequest".into(), "MyHandler".into()),
                "BusError: RequestPipeline handler registered already. Request type 'MyRequest' by handler 'MyHandler'",
            ),
            (
                BusError::RequestHandlerRegistered("MyRequest".into(), "MyHandler".into()),
                "BusError: Request handler registered already. Request type 'MyRequest' by handler 'MyHandler'",
            ),
            (
                BusError::EventHandlerDatabaseRegistered("MyEvent".into()),
                "BusError: EventHandlerDatabase registered already: 'MyEvent'",
            ),
            (
                BusError::EventHandlerDatabaseNotFound("MyEvent".into()),
                "BusError: EventHandlerDatabase not found: 'MyEvent'",
            ),
            (
                BusError::EventDeserializerNotFound("MyEvent".into()),
                "BusError: EventDeserializerNotFound not found: 'MyEvent'",
            ),
            (
                BusError::RequestHandlerNotFound("MyRequest".into()),
                "BusError: No request handler found for request type 'MyRequest'",
            ),
            (
                BusError::EventQueueSettingsNotFoundConfig("queue1".into()),
                "BusError: Not Found queue config by name 'queue1'",
            ),
            (
                BusError::RequestHandlerIncorrectResponseType(
                    "MyRequest".into(),
                    "MyResponse".into(),
                ),
                "BusError: Request handler return incorrect response type for request type 'MyRequest' and response type: 'MyResponse'",
            ),
            (
                BusError::RequestHandlerIncorrectErrorType("MyRequest".into(), "MyResponse".into()),
                "BusError: Request handler return incorrect error type for request type 'MyRequest' and response type: 'MyResponse'",
            ),
            (
                BusError::EventIncorrectRequestType("MyEvent".into(), "MyHandler".into()),
                "BusError: Incorrect Event type 'MyEvent' by handler 'MyHandler'",
            ),
            (
                BusError::EventIncorrectErrorType(
                    "MyError".into(),
                    "MyEvent".into(),
                    "boom".into(),
                ),
                "BusError: Incorrect Error type 'MyError' by event 'MyEvent'. Error: 'boom'",
            ),
            (
                BusError::EventHandlerNotFound("MyEvent".into()),
                "BusError: No event handler found for event type 'MyEvent'",
            ),
            (
                BusError::EventDatabaseIncorrectRequestType("MyEvent".into(), "MyHandler".into()),
                "BusError: Incorrect Event type 'MyEvent' by handler 'MyHandler'",
            ),
            (
                BusError::EventDatabaseHandlerNotFound("MyEvent".into()),
                "BusError: No event handler found for event type 'MyEvent'",
            ),
        ];

        for (err, expected) in cases {
            assert_eq!(err.to_string(), expected);
        }
    }

    #[cfg(any(feature = "sea-orm-postgres", feature = "sea-orm-mysql"))]
    #[test]
    fn test_display_db_variants() {
        use sea_orm::DbErr;

        let db_err = BusError::DbErr(DbErr::Custom("db fail".into()));
        let msg = db_err.to_string();
        assert!(msg.contains("BusError: DbErr"));
        assert!(msg.contains("db fail"));

        let invalid = BusError::InvalidTableName("bad-name".into());
        assert_eq!(
            invalid.to_string(),
            "BusError: Unsupported database name: 'bad-name'"
        );

        let backend = BusError::UnsupportedDatabaseBackend("sqlite".into());
        assert_eq!(
            backend.to_string(),
            "BusError: Unsupported database backend: sqlite"
        );
    }
}
