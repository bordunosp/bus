use crate::core::contracts::{AnyError, IErasedRequestHandler, IRequest, IRequestHandler};
use crate::core::error_bus::BusError;
use crate::core::request_pipeline::{BoxedHandlerWrapper, PipelineWithNext, get_request_pipelines};
use async_trait::async_trait;
use dashmap::DashMap;
use dashmap::mapref::entry::Entry;
use once_cell::sync::OnceCell;
use std::any::Any;
use std::error::Error;
use std::pin::Pin;
use std::sync::Arc;

type AsyncFactory = dyn Fn() -> Pin<
        Box<dyn Future<Output = Result<Box<dyn IErasedRequestHandler>, Box<dyn Error>>> + Send>,
    > + Send
    + Sync;

static REQUEST_HANDLERS: OnceCell<DashMap<&'static str, Arc<AsyncFactory>>> = OnceCell::new();

pub(crate) fn get_handlers() -> &'static DashMap<&'static str, Arc<AsyncFactory>> {
    REQUEST_HANDLERS.get_or_init(DashMap::new)
}

struct RequestHandlerWrapper<H, RQ, RS, TError>
where
    H: IRequestHandler<RQ, RS, TError>,
    RQ: IRequest<RS, TError>,
    RS: Sized + Send + Sync + 'static,
    TError: AnyError,
{
    inner: H,
    _phantom: std::marker::PhantomData<(RQ, RS, TError)>,
}

impl<H, RQ, RS, TError> RequestHandlerWrapper<H, RQ, RS, TError>
where
    H: IRequestHandler<RQ, RS, TError>,
    RQ: IRequest<RS, TError>,
    RS: Sized + Send + Sync + 'static,
    TError: AnyError,
{
    pub fn new(handler: H) -> Self {
        Self {
            inner: handler,
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<H, RQ, RS, TError> IErasedRequestHandler for RequestHandlerWrapper<H, RQ, RS, TError>
where
    H: IRequestHandler<RQ, RS, TError>,
    RQ: IRequest<RS, TError>,
    RS: Sized + Send + Sync + 'static,
    TError: AnyError,
{
    async fn handle(
        &self,
        request: Box<dyn Any + Send + Sync>,
        #[cfg(feature = "cancellation-token")]
        cancellation_token: tokio_util::sync::CancellationToken,
    ) -> Result<Box<dyn Any + Send + Sync>, Box<dyn Error>> {
        let req = request.downcast::<RQ>().map_err(|_| {
            BusError::RequestIncorrectRequestType(
                std::any::type_name::<RQ>().to_string(),
                std::any::type_name::<H>().to_string(),
            )
        })?;
        let result = self
            .inner
            .handle_async(
                *req,
                #[cfg(feature = "cancellation-token")]
                cancellation_token,
            )
            .await?;
        Ok(Box::new(result))
    }
}

pub(crate) fn register_request_handler<H, RQ, RS, TError, F, Fut>(
    factory: F,
) -> Result<(), BusError>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<H, TError>> + Send + 'static,
    H: IRequestHandler<RQ, RS, TError>,
    RQ: IRequest<RS, TError>,
    RS: Send + Sync + 'static,
    TError: AnyError,
{
    let request_type_name = std::any::type_name::<RQ>();
    let handler_type_name = std::any::type_name::<H>();
    let handlers = get_handlers();

    match handlers.entry(request_type_name) {
        Entry::Occupied(_) => Err(BusError::RequestHandlerRegistered(
            request_type_name.to_string(),
            handler_type_name.to_string(),
        )),
        Entry::Vacant(entry) => {
            let erased_factory: Arc<AsyncFactory> = Arc::new(move || {
                let fut = factory();
                Box::pin(async move {
                    let handler = fut.await?;
                    Ok::<_, Box<dyn Error>>(Box::new(RequestHandlerWrapper::new(handler))
                        as Box<dyn IErasedRequestHandler>)
                })
            });
            entry.insert(erased_factory);
            Ok(())
        }
    }
}

pub(crate) async fn handle<TRequest, TResponse, TError>(
    request: TRequest,
    #[cfg(feature = "cancellation-token")] cancellation_token: tokio_util::sync::CancellationToken,
) -> Result<TResponse, TError>
where
    TRequest: IRequest<TResponse, TError>,
    TResponse: Sized + Send + Sync + 'static,
    TError: AnyError,
{
    let request_type_name = std::any::type_name::<TRequest>();
    let factory = get_handlers()
        .get(request_type_name)
        .ok_or(BusError::RequestHandlerNotFound(
            request_type_name.to_string(),
        ))?
        .clone();

    let handler = factory().await.map_err(|err| {
        BusError::RequestHandlerFactoryFailed(
            std::any::type_name::<TRequest>().to_string(),
            err.to_string(),
        )
    })?;

    let mut current_handler: Arc<dyn IErasedRequestHandler> =
        Arc::new(BoxedHandlerWrapper(handler));

    for factory in get_request_pipelines().read().iter() {
        let pipeline = factory();
        let next = Arc::clone(&current_handler);
        current_handler = Arc::new(PipelineWithNext { pipeline, next });
    }

    match current_handler
        .handle(
            Box::new(request),
            #[cfg(feature = "cancellation-token")]
            cancellation_token,
        )
        .await
    {
        Ok(result) => {
            let typed_result = result.downcast::<TResponse>().map_err(|_| {
                BusError::RequestHandlerIncorrectResponseType(
                    request_type_name.to_string(),
                    std::any::type_name::<TResponse>().to_string(),
                )
            })?;
            Ok(*typed_result)
        }
        Err(err) => {
            let typed_error = err.downcast::<TError>().map_err(|_| {
                BusError::RequestHandlerIncorrectErrorType(
                    request_type_name.to_string(),
                    std::any::type_name::<TResponse>().to_string(),
                )
            })?;
            Err(*typed_error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::contracts::{IRequest, IRequestPipeline};
    use crate::core::request_pipeline::register_request_pipeline;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug)]
    struct TestRequest;
    #[derive(Debug, PartialEq)]
    struct TestResponse(&'static str);
    #[derive(Debug)]
    struct TestError;

    impl std::fmt::Display for TestError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "TestError")
        }
    }

    impl Error for TestError {}
    impl From<BusError> for TestError {
        fn from(_: BusError) -> Self {
            TestError
        }
    }

    impl IRequest<TestResponse, TestError> for TestRequest {}

    struct TestHandler {
        pub called: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl IRequestHandler<TestRequest, TestResponse, TestError> for TestHandler {
        async fn handle_async(
            &self,
            _req: TestRequest,
            #[cfg(feature = "cancellation-token")] _: tokio_util::sync::CancellationToken,
        ) -> Result<TestResponse, TestError> {
            self.called.fetch_add(1, Ordering::SeqCst);
            Ok(TestResponse("ok"))
        }
    }

    struct FailingHandler;

    #[async_trait]
    impl IRequestHandler<TestRequest, TestResponse, TestError> for FailingHandler {
        async fn handle_async(
            &self,
            _req: TestRequest,
            #[cfg(feature = "cancellation-token")] _: tokio_util::sync::CancellationToken,
        ) -> Result<TestResponse, TestError> {
            Err(TestError)
        }
    }

    struct TestPipeline {
        pub called: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl IRequestPipeline for TestPipeline {
        async fn handle(
            &self,
            req: Box<dyn Any + Send + Sync>,
            next: Arc<dyn IErasedRequestHandler>,
            #[cfg(feature = "cancellation-token")]
            cancellation_token: tokio_util::sync::CancellationToken,
        ) -> Result<Box<dyn Any + Send + Sync>, Box<dyn Error>> {
            self.called.fetch_add(1, Ordering::SeqCst);
            next.handle(
                req,
                #[cfg(feature = "cancellation-token")]
                cancellation_token,
            )
            .await
        }
    }

    #[tokio::test]
    async fn test_successful_request_handling() {
        get_handlers().clear();
        get_request_pipelines().write().clear();

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        register_request_handler(move || {
            let handler = TestHandler {
                called: counter_clone.clone(),
            };
            async move { Ok(handler) }
        })
        .unwrap();

        let result = handle(
            TestRequest,
            #[cfg(feature = "cancellation-token")]
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(result, TestResponse("ok"));
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_pipeline_is_called() {
        get_handlers().clear();
        get_request_pipelines().write().clear();

        let handler_counter = Arc::new(AtomicUsize::new(0));
        let pipeline_counter = Arc::new(AtomicUsize::new(0));

        register_request_pipeline({
            let pipeline_counter = pipeline_counter.clone();
            move || {
                Arc::new(TestPipeline {
                    called: pipeline_counter.clone(),
                })
            }
        });

        register_request_handler({
            let handler_counter = handler_counter.clone();
            move || {
                let handler = TestHandler {
                    called: handler_counter.clone(),
                };
                async move { Ok(handler) }
            }
        })
        .unwrap();

        let result = handle(
            TestRequest,
            #[cfg(feature = "cancellation-token")]
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();

        assert_eq!(result, TestResponse("ok"));
        assert_eq!(handler_counter.load(Ordering::SeqCst), 1);
        assert_eq!(pipeline_counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_duplicate_handler_registration() {
        get_handlers().clear();

        let _ = register_request_handler(|| async { Ok(FailingHandler) });
        let result = register_request_handler(|| async { Ok(FailingHandler) });

        assert!(matches!(
            result,
            Err(BusError::RequestHandlerRegistered(_, _))
        ));
    }

    #[tokio::test]
    async fn test_handler_not_found() {
        get_handlers().clear();
        get_request_pipelines().write().clear();

        let err = handle::<TestRequest, TestResponse, TestError>(
            TestRequest,
            #[cfg(feature = "cancellation-token")]
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap_err();

        assert!(matches!(err, TestError));
    }

    #[tokio::test]
    async fn test_handler_returns_error() {
        get_handlers().clear();
        get_request_pipelines().write().clear();

        register_request_handler(|| async { Ok(FailingHandler) }).unwrap();

        let err = handle::<TestRequest, TestResponse, TestError>(
            TestRequest,
            #[cfg(feature = "cancellation-token")]
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap_err();

        assert!(matches!(err, TestError));
    }

    #[tokio::test]
    async fn test_factory_fails() {
        get_handlers().clear();
        get_request_pipelines().write().clear();

        register_request_handler(|| async { Err::<TestHandler, TestError>(TestError) }).unwrap();

        let err = handle::<TestRequest, TestResponse, TestError>(
            TestRequest,
            #[cfg(feature = "cancellation-token")]
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap_err();

        assert!(matches!(err, TestError));
    }

    #[tokio::test]
    async fn test_incorrect_response_type() {
        use crate::core::contracts::{IErasedRequestHandler, IRequestHandler};

        #[derive(Debug)]
        struct MyRequest;
        #[derive(Debug)]
        struct ActualResponse;
        #[derive(Debug)]
        struct ExpectedResponse;
        #[derive(Debug)]
        struct MyError;

        impl std::fmt::Display for MyError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "MyError")
            }
        }
        impl Error for MyError {}
        impl From<BusError> for MyError {
            fn from(_: BusError) -> Self {
                MyError
            }
        }

        impl IRequest<ActualResponse, MyError> for MyRequest {}

        struct MyHandler;
        #[async_trait]
        impl IRequestHandler<MyRequest, ActualResponse, MyError> for MyHandler {
            async fn handle_async(
                &self,
                _request: MyRequest,
                #[cfg(feature = "cancellation-token")] _: tokio_util::sync::CancellationToken,
            ) -> Result<ActualResponse, MyError> {
                Ok(ActualResponse)
            }
        }

        let erased: Box<dyn IErasedRequestHandler> =
            Box::new(RequestHandlerWrapper::new(MyHandler));

        let result = erased
            .handle(
                Box::new(MyRequest) as Box<dyn Any + Send + Sync>,
                #[cfg(feature = "cancellation-token")]
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();

        let downcast_result = result.downcast::<ExpectedResponse>();

        assert!(
            downcast_result.is_err(),
            "Expected downcast to fail due to incorrect response type"
        );
    }
}

// ///////////////////////

// #[derive(Debug)]
// pub enum MyError {
//     NotFound,
//     DbError(String),
//     InvalidInput(String),
// }
//
// impl std::fmt::Display for MyError {
//     fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
//         match self {
//             MyError::NotFound => write!(f, "Not found"),
//             MyError::DbError(msg) => write!(f, "Database error: {}", msg),
//             MyError::InvalidInput(msg) => write!(f, "Invalid input: {}", msg),
//         }
//     }
// }
//
// impl std::error::Error for MyError {}
//
// impl From<BusError> for MyError {
//     fn from(err: BusError) -> Self {
//         MyError::DbError(format!("BusError: {}", err))
//     }
// }
//
// pub struct GetUserRequest {
//     pub id: i32,
// }
//
// #[derive(Debug)]
// pub struct GetUserResponse {
//     pub name: String,
//     pub email: String,
// }
//
// impl Debug for GetUserRequest {
//     fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         todo!()
//     }
// }
//
// impl IRequest<GetUserResponse, MyError> for GetUserRequest {}
//
// pub struct GetUserHandler;
//
// #[async_trait]
// impl IRequestHandler<GetUserRequest, GetUserResponse, MyError> for GetUserHandler {
//     async fn handle_async(&self, request: GetUserRequest) -> Result<GetUserResponse, MyError> {
//         if request.id <= 0 {
//             return Err(MyError::InvalidInput("Negative ID".into()));
//         }
//
//         register_request_handlers(|| async {
//             Ok(GetUserHandler)
//         }).await?;
//
//
//         // Типу асинхронний запит до БД
//         Ok(GetUserResponse {
//             name: format!("User #{}", request.id),
//             email: format!("user{}@example.com", request.id),
//         })
//     }
// }
