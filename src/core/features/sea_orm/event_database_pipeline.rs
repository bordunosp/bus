use crate::core::contracts::{IErasedEventHandlerDatabase, IEventDatabasePipeline};
use async_trait::async_trait;
use once_cell::sync::OnceCell;
use parking_lot::RwLock;
use std::any::Any;
use std::error::Error;
use std::sync::Arc;

type PipelineFactory = dyn Fn() -> Arc<dyn IEventDatabasePipeline> + Send + Sync + 'static;

static EVENTS_DATABASE_PIPELINES: OnceCell<RwLock<Vec<Arc<PipelineFactory>>>> = OnceCell::new();

pub(crate) fn get_event_database_pipelines() -> &'static RwLock<Vec<Arc<PipelineFactory>>> {
    EVENTS_DATABASE_PIPELINES.get_or_init(|| RwLock::new(Vec::new()))
}

pub(crate) fn register_event_database_pipeline(
    factory: impl Fn() -> Arc<dyn IEventDatabasePipeline> + Send + Sync + 'static,
) {
    get_event_database_pipelines()
        .write()
        .push(Arc::new(factory));
}

pub(crate) struct DatabasePipelineWrapper {
    pub pipeline: Arc<dyn IEventDatabasePipeline>,
    pub next: Arc<dyn IErasedEventHandlerDatabase>,
}

#[async_trait]
impl IErasedEventHandlerDatabase for DatabasePipelineWrapper {
    async fn handle(
        &self,
        request: Box<dyn Any + Send + Sync>,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.pipeline.handle(request, self.next.clone()).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::contracts::{IErasedEventHandlerDatabase, IEventDatabasePipeline};
    use async_trait::async_trait;
    use std::any::Any;
    use std::error::Error;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub(crate) struct EventDatabasePipelineWithNext {
        pub pipeline: Arc<dyn IEventDatabasePipeline>,
        pub next: Arc<dyn IErasedEventHandlerDatabase>,
    }

    #[async_trait]
    impl IErasedEventHandlerDatabase for EventDatabasePipelineWithNext {
        async fn handle(
            &self,
            request: Box<dyn Any + Send + Sync>,
        ) -> Result<(), Box<dyn Error + Send + Sync>> {
            self.pipeline.handle(request, Arc::clone(&self.next)).await
        }
    }

    pub(crate) struct BoxedDatabaseHandlerWrapper(pub Box<dyn IErasedEventHandlerDatabase>);

    #[async_trait]
    impl IErasedEventHandlerDatabase for BoxedDatabaseHandlerWrapper {
        async fn handle(
            &self,
            request: Box<dyn Any + Send + Sync>,
        ) -> Result<(), Box<dyn Error + Send + Sync>> {
            self.0.handle(request).await
        }
    }

    #[derive(Debug)]
    struct DummyEvent;

    struct DummyHandler {
        pub called: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl IErasedEventHandlerDatabase for DummyHandler {
        async fn handle(
            &self,
            _event: Box<dyn Any + Send + Sync>,
        ) -> Result<(), Box<dyn Error + Send + Sync>> {
            self.called.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    struct DummyPipeline {
        pub called: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl IEventDatabasePipeline for DummyPipeline {
        async fn handle(
            &self,
            event: Box<dyn Any + Send + Sync>,
            next: Arc<dyn IErasedEventHandlerDatabase>,
        ) -> Result<(), Box<dyn Error + Send + Sync>> {
            self.called.fetch_add(1, Ordering::SeqCst);
            next.handle(event).await
        }
    }

    #[tokio::test]
    async fn test_get_and_register_event_database_pipeline() {
        get_event_database_pipelines().write().clear();

        register_event_database_pipeline(|| {
            Arc::new(DummyPipeline {
                called: Arc::new(AtomicUsize::new(0)),
            })
        });

        let pipelines = get_event_database_pipelines().read();
        assert_eq!(pipelines.len(), 1);
    }

    #[tokio::test]
    async fn test_boxed_database_handler_wrapper_delegates() {
        let counter = Arc::new(AtomicUsize::new(0));
        let handler = DummyHandler {
            called: Arc::clone(&counter),
        };

        let wrapper = BoxedDatabaseHandlerWrapper(Box::new(handler));
        let result = wrapper
            .handle(Box::new(DummyEvent) as Box<dyn Any + Send + Sync>)
            .await;

        assert!(result.is_ok());
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_event_database_pipeline_with_next_calls_both() {
        let pipeline_counter = Arc::new(AtomicUsize::new(0));
        let handler_counter = Arc::new(AtomicUsize::new(0));

        let pipeline = DummyPipeline {
            called: Arc::clone(&pipeline_counter),
        };

        let handler = DummyHandler {
            called: Arc::clone(&handler_counter),
        };

        let composed = EventDatabasePipelineWithNext {
            pipeline: Arc::new(pipeline),
            next: Arc::new(handler),
        };

        let result = composed
            .handle(Box::new(DummyEvent) as Box<dyn Any + Send + Sync>)
            .await;

        assert!(result.is_ok());
        assert_eq!(pipeline_counter.load(Ordering::SeqCst), 1);
        assert_eq!(handler_counter.load(Ordering::SeqCst), 1);
    }
}
