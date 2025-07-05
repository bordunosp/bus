use crate::core::contracts::{IErasedRequestHandler, IRequestPipeline};
use async_trait::async_trait;
use once_cell::sync::OnceCell;
use parking_lot::RwLock;
use std::any::Any;
use std::error::Error;
use std::sync::Arc;

type PipelineFactory = dyn Fn() -> Arc<dyn IRequestPipeline> + Send + Sync + 'static;

static REQUEST_PIPELINES: OnceCell<RwLock<Vec<Arc<PipelineFactory>>>> = OnceCell::new();

pub(crate) fn get_request_pipelines() -> &'static RwLock<Vec<Arc<PipelineFactory>>> {
    REQUEST_PIPELINES.get_or_init(|| RwLock::new(Vec::new()))
}

pub(crate) fn register_request_pipeline(
    factory: impl Fn() -> Arc<dyn IRequestPipeline> + Send + Sync + 'static,
) {
    get_request_pipelines().write().push(Arc::new(factory));
}

pub(crate) struct BoxedHandlerWrapper(pub Box<dyn IErasedRequestHandler>);

#[async_trait]
impl IErasedRequestHandler for BoxedHandlerWrapper {
    async fn handle(
        &self,
        request: Box<dyn Any + Send + Sync>,
        #[cfg(feature = "cancellation-token")]
        cancellation_token: tokio_util::sync::CancellationToken,
    ) -> Result<Box<dyn Any + Send + Sync>, Box<dyn Error>> {
        self.0
            .handle(
                request,
                #[cfg(feature = "cancellation-token")]
                cancellation_token,
            )
            .await
    }
}

pub(crate) struct PipelineWithNext {
    pub pipeline: Arc<dyn IRequestPipeline>,
    pub next: Arc<dyn IErasedRequestHandler>,
}

#[async_trait]
impl IErasedRequestHandler for PipelineWithNext {
    async fn handle(
        &self,
        request: Box<dyn Any + Send + Sync>,
        #[cfg(feature = "cancellation-token")]
        cancellation_token: tokio_util::sync::CancellationToken,
    ) -> Result<Box<dyn Any + Send + Sync>, Box<dyn Error>> {
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
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct DummyHandler {
        pub called: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl IErasedRequestHandler for DummyHandler {
        async fn handle(
            &self,
            request: Box<dyn Any + Send + Sync>,
            #[cfg(feature = "cancellation-token")] _: tokio_util::sync::CancellationToken,
        ) -> Result<Box<dyn Any + Send + Sync>, Box<dyn Error>> {
            self.called.fetch_add(1, Ordering::SeqCst);
            Ok(request)
        }
    }

    struct DummyPipeline {
        pub called: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl IRequestPipeline for DummyPipeline {
        async fn handle(
            &self,
            request: Box<dyn Any + Send + Sync>,
            next: Arc<dyn IErasedRequestHandler>,
            #[cfg(feature = "cancellation-token")]
            cancellation_token: tokio_util::sync::CancellationToken,
        ) -> Result<Box<dyn Any + Send + Sync>, Box<dyn Error>> {
            self.called.fetch_add(1, Ordering::SeqCst);
            next.handle(
                request,
                #[cfg(feature = "cancellation-token")]
                cancellation_token,
            )
            .await
        }
    }

    #[tokio::test]
    async fn test_register_and_get_request_pipelines() {
        let pipelines = get_request_pipelines();
        pipelines.write().clear();

        register_request_pipeline(|| {
            Arc::new(DummyPipeline {
                called: Arc::new(AtomicUsize::new(0)),
            })
        });

        let pipelines = get_request_pipelines();
        assert_eq!(pipelines.read().len(), 1);
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
                Box::new("test".to_string()) as Box<dyn Any + Send + Sync>,
                #[cfg(feature = "cancellation-token")]
                tokio_util::sync::CancellationToken::new(),
            )
            .await;

        assert!(result.is_ok());
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_pipeline_with_next_calls_both() {
        let pipeline_counter = Arc::new(AtomicUsize::new(0));
        let handler_counter = Arc::new(AtomicUsize::new(0));

        let pipeline = DummyPipeline {
            called: Arc::clone(&pipeline_counter),
        };

        let handler = DummyHandler {
            called: Arc::clone(&handler_counter),
        };

        let composed = PipelineWithNext {
            pipeline: Arc::new(pipeline),
            next: Arc::new(handler),
        };

        let result = composed
            .handle(
                Box::new(123_i32) as Box<dyn Any + Send + Sync>,
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
// impl IRequestPipeline for LoggingPipeline {
//     async fn handle(
//         &self,
//         request: Box<dyn Any + Send + Sync>,
//         next: Arc<dyn IErasedRequestHandler>,
//     ) -> Result<Box<dyn Any + Send + Sync>, Box<dyn Error>> {
//
//
//
//         register_request_pipeline(|| Arc::new(LoggingPipeline));
//
//         println!("Pipeline: Request received");
//         let result = next.handle(request).await;
//         println!("Pipeline: Request processed");
//         result
//     }
// }
