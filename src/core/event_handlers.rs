use crate::core::contracts::{AnyError, IErasedEventHandler, IEvent, IEventHandler};
use crate::core::error_bus::BusError;
use crate::core::event_pipeline::{
    BoxedHandlerWrapper, EventPipelineWithNext, get_event_pipelines,
};
use async_trait::async_trait;
use dashmap::DashMap;
use futures::StreamExt;
use futures::stream::FuturesUnordered;
use once_cell::sync::OnceCell;
use std::any::Any;
use std::error::Error;
use std::pin::Pin;
use std::sync::Arc;

type AsyncEventFactory = dyn Fn()
        -> Pin<Box<dyn Future<Output = Result<Box<dyn IErasedEventHandler>, Box<dyn Error>>> + Send>>
    + Send
    + Sync;

static EVENT_HANDLER_FACTORIES: OnceCell<DashMap<&'static str, Vec<Arc<AsyncEventFactory>>>> =
    OnceCell::new();

pub(crate) fn get_handlers() -> &'static DashMap<&'static str, Vec<Arc<AsyncEventFactory>>> {
    EVENT_HANDLER_FACTORIES.get_or_init(DashMap::new)
}

pub struct EventHandlerWrapper<H, E, TError>
where
    H: IEventHandler<E, TError>,
    E: IEvent<TError>,
    TError: AnyError,
{
    inner: H,
    _phantom: std::marker::PhantomData<(E, TError)>,
}

impl<H, E, TError> EventHandlerWrapper<H, E, TError>
where
    H: IEventHandler<E, TError>,
    E: IEvent<TError>,
    TError: AnyError,
{
    pub fn new(inner: H) -> Self {
        Self {
            inner,
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<H, E, TError> IErasedEventHandler for EventHandlerWrapper<H, E, TError>
where
    H: IEventHandler<E, TError>,
    E: IEvent<TError>,
    TError: AnyError,
{
    async fn handle(
        &self,
        event: Box<dyn Any + Send + Sync>,
        #[cfg(feature = "cancellation-token")]
        cancellation_token: tokio_util::sync::CancellationToken,
    ) -> Result<(), Box<dyn Error>> {
        let evt = event.downcast::<E>().map_err(|_| {
            BusError::EventIncorrectRequestType(
                std::any::type_name::<E>().to_string(),
                std::any::type_name::<H>().to_string(),
            )
        })?;
        self.inner
            .handle_async(
                *evt,
                #[cfg(feature = "cancellation-token")]
                cancellation_token,
            )
            .await?;
        Ok(())
    }
}

pub async fn publish<TEvent, TError>(
    event: TEvent,
    #[cfg(feature = "cancellation-token")] cancellation_token: tokio_util::sync::CancellationToken,
) -> Result<(), TError>
where
    TEvent: IEvent<TError>,
    TError: AnyError,
{
    let type_name = std::any::type_name::<TEvent>();
    let handlers = get_handlers();

    let Some(factories) = handlers.get(type_name) else {
        return Ok(());
    };

    let mut futures = FuturesUnordered::new();

    for factory in factories.iter() {
        let base_handler = factory().await.map_err(|err| {
            let err_str = err.to_string();
            err.downcast::<TError>().map(|e| *e).unwrap_or_else(|_| {
                TError::from(BusError::EventIncorrectErrorType(
                    std::any::type_name::<TError>().to_string(),
                    type_name.to_string(),
                    err_str,
                ))
            })
        })?;

        let mut current_handler: Arc<dyn IErasedEventHandler> =
            Arc::new(BoxedHandlerWrapper(base_handler));

        for pipeline_factory in get_event_pipelines().read().iter() {
            let pipeline = pipeline_factory();
            let next = Arc::clone(&current_handler);
            current_handler = Arc::new(EventPipelineWithNext { pipeline, next });
        }

        #[cfg(feature = "cancellation-token")]
        let cancellation_token_clone = cancellation_token.clone();
        let cloned = event.clone();

        futures.push(async move {
            current_handler
                .handle(
                    Box::new(cloned),
                    #[cfg(feature = "cancellation-token")]
                    cancellation_token_clone,
                )
                .await
                .map_err(|err| {
                    let err_str = err.to_string();
                    err.downcast::<TError>().map(|e| *e).unwrap_or_else(|_| {
                        TError::from(BusError::EventIncorrectErrorType(
                            std::any::type_name::<TError>().to_string(),
                            type_name.to_string(),
                            err_str,
                        ))
                    })
                })
        });
    }

    while let Some(result) = futures.next().await {
        if let Err(e) = result {
            return Err(e);
        }
    }

    Ok(())
}

pub(crate) fn register_event_handler<H, E, TError, F, Fut>(factory: F) -> Result<(), BusError>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<H, TError>> + Send + 'static,
    H: IEventHandler<E, TError> + 'static,
    E: IEvent<TError>,
    TError: AnyError,
{
    let event_type_name = std::any::type_name::<E>();
    let handlers = get_handlers();

    let erased: Arc<AsyncEventFactory> = Arc::new(move || {
        let fut = factory();
        Box::pin(async move {
            let handler = fut.await?;
            Ok::<_, Box<dyn Error>>(
                Box::new(EventHandlerWrapper::new(handler)) as Box<dyn IErasedEventHandler>
            )
        })
    });

    handlers
        .entry(event_type_name)
        .or_insert_with(Vec::new)
        .push(erased);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::contracts::*;
    use crate::core::event_pipeline::{get_event_pipelines, register_event_pipeline};
    use async_trait::async_trait;
    use std::any::Any;
    use std::error::Error;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
    struct DummyEvent;
    #[derive(Debug)]
    struct DummyError;

    impl std::fmt::Display for DummyError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "DummyError")
        }
    }

    impl Error for DummyError {}
    impl From<BusError> for DummyError {
        fn from(_: BusError) -> Self {
            DummyError
        }
    }

    impl IEvent<DummyError> for DummyEvent {}

    struct DummyHandler {
        pub called: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl IEventHandler<DummyEvent, DummyError> for DummyHandler {
        async fn handle_async(
            &self,
            _event: DummyEvent,
            #[cfg(feature = "cancellation-token")] _: tokio_util::sync::CancellationToken,
        ) -> Result<(), DummyError> {
            self.called.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    struct FailingHandler;

    #[async_trait]
    impl IEventHandler<DummyEvent, DummyError> for FailingHandler {
        async fn handle_async(
            &self,
            _event: DummyEvent,
            #[cfg(feature = "cancellation-token")] _: tokio_util::sync::CancellationToken,
        ) -> Result<(), DummyError> {
            Err(DummyError)
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
    async fn test_register_and_publish_success() {
        get_handlers().clear();
        get_event_pipelines().write().clear();

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        register_event_handler(move || {
            let handler = DummyHandler {
                called: counter_clone.clone(),
            };
            async move { Ok(handler) }
        })
        .unwrap();

        let result = publish(
            DummyEvent,
            #[cfg(feature = "cancellation-token")]
            tokio_util::sync::CancellationToken::new(),
        )
        .await;
        assert!(result.is_ok());
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_publish_with_pipeline() {
        get_handlers().clear();
        get_event_pipelines().write().clear();

        let handler_counter = Arc::new(AtomicUsize::new(0));
        let pipeline_counter = Arc::new(AtomicUsize::new(0));

        register_event_handler({
            let handler_counter = handler_counter.clone();
            move || {
                let handler = DummyHandler {
                    called: handler_counter.clone(),
                };
                async move { Ok(handler) }
            }
        })
        .unwrap();

        register_event_pipeline({
            let pipeline_counter = pipeline_counter.clone();
            move || {
                Arc::new(DummyPipeline {
                    called: pipeline_counter.clone(),
                })
            }
        });

        let result = publish(
            DummyEvent,
            #[cfg(feature = "cancellation-token")]
            tokio_util::sync::CancellationToken::new(),
        )
        .await;
        assert!(result.is_ok());
        assert_eq!(handler_counter.load(Ordering::SeqCst), 1);
        assert_eq!(pipeline_counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_publish_with_handler_error() {
        get_handlers().clear();
        get_event_pipelines().write().clear();

        register_event_handler(|| async { Ok(FailingHandler) }).unwrap();

        let result = publish(
            DummyEvent,
            #[cfg(feature = "cancellation-token")]
            tokio_util::sync::CancellationToken::new(),
        )
        .await;
        assert!(matches!(result, Err(DummyError)));
    }

    #[tokio::test]
    async fn test_event_handler_wrapper_type_mismatch() {
        let handler = DummyHandler {
            called: Arc::new(AtomicUsize::new(0)),
        };

        let wrapper = EventHandlerWrapper::new(handler);
        let result = wrapper
            .handle(
                Box::new("not an event") as Box<dyn Any + Send + Sync>,
                #[cfg(feature = "cancellation-token")]
                tokio_util::sync::CancellationToken::new(),
            )
            .await;

        assert!(matches!(
            result.unwrap_err().downcast_ref::<BusError>(),
            Some(BusError::EventIncorrectRequestType(_, _))
        ));
    }

    #[tokio::test]
    async fn test_publish_with_factory_error() {
        get_handlers().clear();
        get_event_pipelines().write().clear();

        register_event_handler(|| async { Err::<DummyHandler, DummyError>(DummyError) }).unwrap();

        let result = publish(
            DummyEvent,
            #[cfg(feature = "cancellation-token")]
            tokio_util::sync::CancellationToken::new(),
        )
        .await;
        assert!(matches!(result, Err(DummyError)));
    }
}

//////////////

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
//
// #[derive(Clone, Debug)]
// pub struct UserCreatedEvent {
//     pub user_id: i32,
//     pub email: String,
// }
//
// impl IEvent<MyError> for UserCreatedEvent {}
//
// pub struct WelcomeEmailSender;
//
// #[async_trait]
// impl IEventHandler<UserCreatedEvent, MyError> for WelcomeEmailSender {
//     async fn handle_async(&self, event: UserCreatedEvent) -> Result<(), MyError> {
//
//         register_event_handlers(|| async {
//             Ok(WelcomeEmailSender)
//         }).await?;
//
//
//         println!(
//             "📧 Sending welcome email to user {} at {}",
//             event.user_id, event.email
//         );
//         // simulate send
//         Ok(())
//     }
// }
