use crate::core::contracts::{IErasedEventHandler, IEventPipeline};
use async_trait::async_trait;
use once_cell::sync::OnceCell;
use parking_lot::RwLock;
use std::any::Any;
use std::error::Error;
use std::sync::Arc;

type PipelineFactory = dyn Fn() -> Arc<dyn IEventPipeline> + Send + Sync + 'static;

static EVENTS_PIPELINES: OnceCell<RwLock<Vec<Arc<PipelineFactory>>>> = OnceCell::new();

pub(crate) fn get_event_pipelines() -> &'static RwLock<Vec<Arc<PipelineFactory>>> {
    EVENTS_PIPELINES.get_or_init(|| RwLock::new(Vec::new()))
}

pub(crate) fn register_event_pipeline(
    factory: impl Fn() -> Arc<dyn IEventPipeline> + Send + Sync + 'static,
) {
    get_event_pipelines().write().push(Arc::new(factory));
}

pub(crate) struct BoxedHandlerWrapper(pub Box<dyn IErasedEventHandler>);

#[async_trait]
impl IErasedEventHandler for BoxedHandlerWrapper {
    async fn handle(
        &self,
        request: Box<dyn Any + Send + Sync>,
        #[cfg(feature = "cancellation-token")]
        cancellation_token: tokio_util::sync::CancellationToken,
    ) -> Result<(), Box<dyn Error>> {
        self.0
            .handle(
                request,
                #[cfg(feature = "cancellation-token")]
                cancellation_token,
            )
            .await
    }
}

pub(crate) struct EventPipelineWithNext {
    pub pipeline: Arc<dyn IEventPipeline>,
    pub next: Arc<dyn IErasedEventHandler>,
}

#[async_trait]
impl IErasedEventHandler for EventPipelineWithNext {
    async fn handle(
        &self,
        request: Box<dyn Any + Send + Sync>,
        #[cfg(feature = "cancellation-token")]
        cancellation_token: tokio_util::sync::CancellationToken,
    ) -> Result<(), Box<dyn Error>> {
        self.pipeline
            .handle(
                request,
                Arc::clone(&self.next),
                #[cfg(feature = "cancellation-token")]
                cancellation_token,
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::contracts::{IErasedEventHandler, IEventPipeline};
    use async_trait::async_trait;
    use std::any::Any;
    use std::error::Error;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug)]
    struct DummyEvent;

    struct DummyHandler {
        pub called: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl IErasedEventHandler for DummyHandler {
        async fn handle(
            &self,
            _event: Box<dyn Any + Send + Sync>,
            #[cfg(feature = "cancellation-token")]
            _cancellation_token: tokio_util::sync::CancellationToken,
        ) -> Result<(), Box<dyn Error>> {
            self.called.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    struct DummyPipeline {
        pub called: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl IEventPipeline for DummyPipeline {
        async fn handle(
            &self,
            event: Box<dyn Any + Send + Sync>,
            next: Arc<dyn IErasedEventHandler>,
            #[cfg(feature = "cancellation-token")]
            cancellation_token: tokio_util::sync::CancellationToken,
        ) -> Result<(), Box<dyn Error>> {
            self.called.fetch_add(1, Ordering::SeqCst);
            next.handle(
                event,
                #[cfg(feature = "cancellation-token")]
                cancellation_token,
            )
            .await
        }
    }

    #[tokio::test]
    async fn test_get_and_register_event_pipeline() {
        get_event_pipelines().write().clear();

        register_event_pipeline(|| {
            Arc::new(DummyPipeline {
                called: Arc::new(AtomicUsize::new(0)),
            })
        });

        let pipelines = get_event_pipelines().read();
        assert_eq!(pipelines.len(), 1);
    }

    #[tokio::test]
    async fn test_boxed_handler_wrapper_delegates() {
        let counter = Arc::new(AtomicUsize::new(0));
        let handler = DummyHandler {
            called: Arc::clone(&counter),
        };

        let wrapper = BoxedHandlerWrapper(Box::new(handler));
        let result = wrapper
            .handle(
                Box::new(DummyEvent) as Box<dyn Any + Send + Sync>,
                #[cfg(feature = "cancellation-token")]
                tokio_util::sync::CancellationToken::new(),
            )
            .await;

        assert!(result.is_ok());
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_event_pipeline_with_next_calls_both() {
        let pipeline_counter = Arc::new(AtomicUsize::new(0));
        let handler_counter = Arc::new(AtomicUsize::new(0));

        let pipeline = DummyPipeline {
            called: Arc::clone(&pipeline_counter),
        };

        let handler = DummyHandler {
            called: Arc::clone(&handler_counter),
        };

        let composed = EventPipelineWithNext {
            pipeline: Arc::new(pipeline),
            next: Arc::new(handler),
        };

        let result = composed
            .handle(
                Box::new(DummyEvent) as Box<dyn Any + Send + Sync>,
                #[cfg(feature = "cancellation-token")]
                tokio_util::sync::CancellationToken::new(),
            )
            .await;

        assert!(result.is_ok());
        assert_eq!(pipeline_counter.load(Ordering::SeqCst), 1);
        assert_eq!(handler_counter.load(Ordering::SeqCst), 1);
    }
}

// Example pipeline implementation
// =================================================================

// struct LoggingPipeline;
//
// #[async_trait]
// impl IEventPipeline for LoggingPipeline {
//     async fn handle(
//         &self,
//         request: Box<dyn Any + Send + Sync>,
//         next: Arc<dyn IErasedEventHandler>,
//     ) -> Result<(), Box<dyn Error>> {
//
//
//
//         register_event_pipeline(|| Arc::new(LoggingPipeline));
//
//         println!("Pipeline: Event received");
//         let result = next.handle(request).await;
//         println!("Pipeline: Event processed");
//         result
//     }
// }
