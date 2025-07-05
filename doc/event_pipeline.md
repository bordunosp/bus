# 🔁 Synchronous Event Pipelines in `bus`

#### This guide explains how to define and register middleware-style pipelines for `in-memory` events in the `bus` framework. These pipelines allow you to intercept and extend the behavior of event handlers — for logging, validation, tracing, metrics, and more.

---

# ✅ What is an IEventPipeline?

### An `IEventPipeline` is a composable unit that wraps around an `in-memory` event handler. Although the handler is asynchronous (`async fn`), it is executed immediately during event publishing — without queueing or background workers.

* ⏱ Measure execution time
* 🧾 Log inputs and outputs
* 🧪 Validate or transform events
* 📊 Emit metrics or traces
* 🔁 Chain multiple behaviors together

Each pipeline implements the trait:

```rust
#[async_trait]
pub trait IEventPipeline: Send + Sync {
    async fn handle(
        &self,
        event: Box<dyn Any + Send + Sync>,
        next: Arc<dyn IErasedEventHandler>,
    ) -> Result<(), Box<dyn Error>>;
}
```

# 🛠 Step 1: Define a Pipeline

Here’s an example of a simple logging pipeline:

```rust
use async_trait::async_trait;
use std::any::Any;
use std::error::Error;
use std::sync::Arc;
use std::time::Instant;
use bus::core::contracts::{IEventPipeline, IErasedEventHandler};

#[derive(Default)]
pub struct LoggingPipeline;

#[async_trait]
#[bus::registry::BusEventPipeline]
impl IEventPipeline for LoggingPipeline {
    async fn handle(
        &self,
        event: Box<dyn Any + Send + Sync>,
        next: Arc<dyn IErasedEventHandler>,
    ) -> Result<(), Box<dyn Error>> {
        let start = Instant::now();
        let result = next.handle(event).await;
        let elapsed = start.elapsed();

        log::info!("🧭 Sync handler executed in {:?}", elapsed);
        result
    }
}
```

# 🧩 Step 2: Register the Pipeline

You can register the pipeline manually:

```rust
use bus::core::registry::event_pipeline;

event_pipeline(|| Arc::new(LoggingPipeline::default()));
```

Or automatically using the procedural macro:

```rust
#[derive(Default)]
#[bus::registry::BusEventPipeline]
pub struct LoggingPipeline;

#[async_trait]
impl IEventPipeline for LoggingPipeline {
    // same as above
}
```

The macro generates a `#[ctor::ctor]` function that registers the pipeline at startup.

# 🔁 Step 3: Pipeline Execution Order

### All registered pipelines are composed into a stack. The last registered pipeline wraps the handler first.

For example:

```
event_pipeline(|| Arc::new(MetricsPipeline));
event_pipeline(|| Arc::new(LoggingPipeline));
```

Execution order:

```
MetricsPipeline → LoggingPipeline → ActualHandler
```

Each pipeline receives the event and a reference to the next handler in the chain.

# 🧪 Example: Event + Handler

```rust
#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub struct MyEvent {
    pub name: String,
}

impl IEvent<AppError> for MyEvent {}

#[derive(Default)]
pub struct MyEventHandler;

#[async_trait]
#[bus::registry::BusEventHandler]
impl IEventHandler<MyEvent, AppError> for MyEventHandler {
    async fn handle_async(&self, event: MyEvent) -> Result<(), AppError> {
        println!("event.name: {}", event.name);
        Ok(())
    }
}
```

# 🚀 Publishing the Event

```rust
bus::publish(MyEvent {
    name: "foo".to_owned(),
})
.await
.unwrap();
```

---

# 🧷 Optional: Graceful Shutdown with `cancellation-token`

### If your application enables the `cancellation-token` feature in bus, each pipeline and handler will receive a `tokio_util::sync::CancellationToken` that can be used to gracefully abort long-running operations.

## ✅ Enabling the feature

In your `Cargo.toml:`

```toml
[dependencies.bus]
version = "..."
features = ["cancellation-token"]
```

## 🧠 How it works

When the feature is enabled:

* The `handle` method of `IEventPipeline` and `IEventHandler` receives an extra argument:

```rust
#[async_trait]
pub trait IEventPipeline {
    async fn handle(
        &self,
        event: Box<dyn Any + Send + Sync>,
        next: Arc<dyn IErasedEventHandler>,
        cancellation_token: CancellationToken, // <== only if feature is enabled
    ) -> Result<(), Box<dyn Error>>;
}
```

* You can use this token to:
  * Cancel HTTP requests
  * Abort retries
  * Exit loops early
  * Respect shutdown signals


## 🧪 Example: Respecting cancellation

```rust
#[async_trait]
impl IEventPipeline for LoggingPipeline {
    async fn handle(
        &self,
        event: Box<dyn Any + Send + Sync>,
        next: Arc<dyn IErasedEventHandler>,
        #[cfg(feature = "cancellation-token")]
        cancellation_token: tokio_util::sync::CancellationToken,
    ) -> Result<(), Box<dyn Error>> {
        if cancellation_token.is_cancelled() {
            log::warn!("⚠️ Cancellation requested before handler started");
            return Ok(());
        }

        let result = next
            .handle(
                event,
                #[cfg(feature = "cancellation-token")]
                cancellation_token.clone(),
            )
            .await;

        result
    }
}
```

## 🧼 Summary


| Step      | What to do   |
|:----------|:-------------|
| 1️⃣	    | Implement IEventPipeline for your struct  |
| 2️⃣	    | Register it using event_pipeline(...) or #[BusEventPipeline]    |
| 3️⃣	    | Use it to wrap event handlers with logging, validation, etc.    |
| 4️⃣	    | Pipelines are executed in reverse registration order    |
| 5️⃣	    | Pipelines are automatically applied to all in-memory event handlers    |


---

## 🧠 Best Practices

1. [x] Keep pipelines focused and composable
2. [x] Use #[derive(Default)] to simplify macro registration
3. [x] Use feature flags like logging or metrics to toggle behavior
4. [x] Avoid panics — always return Result
5. [x] Use #[cfg(feature = "cancellation-token")] to support graceful shutdown

---

## 🧠 Best Practices with Cancellation

1. [x] Always check `cancellation_token.is_cancelled()` before starting expensive work
2. [x] Pass the token to any async sub-tasks or services
3. [x] Use `.cancelled().await` to await cancellation reactively
4. [x] Clone the token if you spawn subtasks

