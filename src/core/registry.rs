use crate::core::contracts::{
    AnyError, IEvent, IEventHandler, IEventPipeline, IRequest, IRequestHandler, IRequestPipeline,
};
use crate::core::error_bus::BusError;
use crate::core::{event_handlers, event_pipeline, request_handlers, request_pipeline};
use std::sync::Arc;

#[cfg(any(feature = "sea-orm-postgres", feature = "sea-orm-mysql"))]
use {
    crate::core::contracts::IEventDatabasePipeline, crate::core::contracts::IEventHandlerDatabase,
    crate::core::features::sea_orm::event_database_pipeline,
    crate::core::features::sea_orm::event_handlers_database,
};

pub fn request_handler<H, RQ, RS, TError, F, Fut>(factory: F) -> Result<(), BusError>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<H, TError>> + Send + 'static,
    H: IRequestHandler<RQ, RS, TError>,
    RQ: IRequest<RS, TError>,
    RS: Send + Sync + 'static,
    TError: AnyError,
{
    request_handlers::register_request_handler(factory)
}

pub fn request_pipeline(factory: impl Fn() -> Arc<dyn IRequestPipeline> + Send + Sync + 'static) {
    request_pipeline::register_request_pipeline(factory)
}

pub fn event_handler<H, E, TError, F, Fut>(factory: F) -> Result<(), BusError>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<H, TError>> + Send + 'static,
    H: IEventHandler<E, TError>,
    E: IEvent<TError>,
    TError: AnyError,
{
    event_handlers::register_event_handler(factory)
}

pub fn event_pipeline(factory: impl Fn() -> Arc<dyn IEventPipeline> + Send + Sync + 'static) {
    event_pipeline::register_event_pipeline(factory)
}

#[cfg(any(feature = "sea-orm-postgres", feature = "sea-orm-mysql"))]
pub fn event_database_handler<H, E, TError, F, Fut>(factory: F) -> Result<(), BusError>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<H, TError>> + Send + 'static,
    H: IEventHandlerDatabase<E, TError>,
    E: IEvent<TError>,
    TError: AnyError,
{
    event_handlers_database::register_event_database_handler(factory)
}

#[cfg(any(feature = "sea-orm-postgres", feature = "sea-orm-mysql"))]
pub fn event_database_pipeline(
    factory: impl Fn() -> Arc<dyn IEventDatabasePipeline> + Send + Sync + 'static,
) {
    event_database_pipeline::register_event_database_pipeline(factory)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::contracts::*;
    use async_trait::async_trait;
    use std::any::Any;
    use std::sync::Arc;

    #[derive(Debug)]
    struct DummyRequest;
    #[derive(Debug, PartialEq)]
    struct DummyResponse(&'static str);
    #[derive(Debug)]
    struct DummyError;

    impl std::fmt::Display for DummyError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "DummyError")
        }
    }
    impl std::error::Error for DummyError {}
    impl From<BusError> for DummyError {
        fn from(_: BusError) -> Self {
            DummyError
        }
    }

    impl IRequest<DummyResponse, DummyError> for DummyRequest {}

    struct DummyHandler;
    #[async_trait]
    impl IRequestHandler<DummyRequest, DummyResponse, DummyError> for DummyHandler {
        async fn handle_async(
            &self,
            _req: DummyRequest,
            #[cfg(feature = "cancellation-token")] _: tokio_util::sync::CancellationToken,
        ) -> Result<DummyResponse, DummyError> {
            Ok(DummyResponse("ok"))
        }
    }

    struct DummyPipeline;
    #[async_trait]
    impl IRequestPipeline for DummyPipeline {
        async fn handle(
            &self,
            req: Box<dyn Any + Send + Sync>,
            next: Arc<dyn IErasedRequestHandler>,
            #[cfg(feature = "cancellation-token")]
            cancellation_token: tokio_util::sync::CancellationToken,
        ) -> Result<Box<dyn Any + Send + Sync>, Box<dyn std::error::Error>> {
            next.handle(
                req,
                #[cfg(feature = "cancellation-token")]
                cancellation_token,
            )
            .await
        }
    }

    #[tokio::test]
    async fn test_request_handler_registration() {
        request_handlers::get_handlers().clear();

        let result = request_handler(|| async { Ok(DummyHandler) });
        assert!(result.is_ok());

        let result = request_handler(|| async { Ok(DummyHandler) });
        assert!(matches!(
            result,
            Err(BusError::RequestHandlerRegistered(_, _))
        ));
    }

    #[test]
    fn test_request_pipeline_registration() {
        request_pipeline::get_request_pipelines().write().clear();

        request_pipeline(|| Arc::new(DummyPipeline));
        assert_eq!(request_pipeline::get_request_pipelines().read().len(), 1);
    }

    #[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
    struct DummyEvent;
    impl IEvent<DummyError> for DummyEvent {}

    struct DummyEventHandler;
    #[async_trait]
    impl IEventHandler<DummyEvent, DummyError> for DummyEventHandler {
        async fn handle_async(
            &self,
            _event: DummyEvent,
            #[cfg(feature = "cancellation-token")] _: tokio_util::sync::CancellationToken,
        ) -> Result<(), DummyError> {
            Ok(())
        }
    }

    struct DummyEventPipeline;
    #[async_trait]
    impl IEventPipeline for DummyEventPipeline {
        async fn handle(
            &self,
            _event: Box<dyn Any + Send + Sync>,
            next: Arc<dyn IErasedEventHandler>,
            #[cfg(feature = "cancellation-token")]
            cancellation_token: tokio_util::sync::CancellationToken,
        ) -> Result<(), Box<dyn std::error::Error>> {
            next.handle(
                Box::new(DummyEvent),
                #[cfg(feature = "cancellation-token")]
                cancellation_token,
            )
            .await
        }
    }

    #[test]
    fn test_event_pipeline_registration() {
        event_pipeline::get_event_pipelines().write().clear();

        event_pipeline(|| Arc::new(DummyEventPipeline));
        assert_eq!(event_pipeline::get_event_pipelines().read().len(), 1);
    }

    #[tokio::test]
    async fn test_event_handler_registration() {
        event_handlers::get_handlers().clear();

        let result = event_handler(|| async { Ok(DummyEventHandler) });
        assert!(result.is_ok());
    }

    #[cfg(any(feature = "sea-orm-postgres", feature = "sea-orm-mysql"))]
    mod sea_orm_tests {
        use super::*;
        use crate::core::features::sea_orm::event_handlers_database::handler_registry;

        struct DummyDbHandler;
        #[async_trait]
        impl IEventHandlerDatabase<DummyEvent, DummyError> for DummyDbHandler {
            fn settings() -> EventQueueSettings {
                EventQueueSettings {
                    queue_name: "test".to_string(),
                    max_retries: 3,
                    execution_timeout: chrono::Duration::seconds(30),
                }
            }

            fn calculate_scheduled_at(_retry: i32) -> chrono::Duration {
                chrono::Duration::minutes(23)
            }

            async fn handle_async(&self, _event: DummyEvent) -> Result<(), DummyError> {
                Ok(())
            }
        }

        struct DummyDbPipeline;
        #[async_trait]
        impl IEventDatabasePipeline for DummyDbPipeline {
            async fn handle(
                &self,
                _event: Box<dyn Any + Send + Sync>,
                next: Arc<dyn IErasedEventHandlerDatabase>,
            ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
                next.handle(Box::new(DummyEvent)).await
            }
        }

        #[tokio::test]
        async fn test_event_database_handler_registration() {
            handler_registry().clear();

            let result = event_database_handler(|| async { Ok(DummyDbHandler) });
            assert!(result.is_ok());
        }

        #[test]
        fn test_event_database_pipeline_registration() {
            use crate::core::features::sea_orm::event_database_pipeline::get_event_database_pipelines;

            get_event_database_pipelines().write().clear();

            event_database_pipeline(|| Arc::new(DummyDbPipeline));
            assert_eq!(get_event_database_pipelines().read().len(), 1);
        }
    }
}
