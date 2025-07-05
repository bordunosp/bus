# 🔄 Request Pipelines in `bus`

### This guide explains how to define and register middleware-style pipelines for request/response (`Query` && `Command`) interactions in the `bus` framework. These pipelines allow you to intercept, transform, and extend the behavior of request handlers — for logging, validation, tracing, metrics, and more.

---

# ✅ What is an `IRequestPipeline`?

#### An `IRequestPipeline` is a composable unit that wraps around a request handler. It can:

* ⏱ Measure execution time
* 🧾 Log inputs and outputs
* 🧪 Validate or transform requests
* 📊 Emit metrics or traces
* 🔁 Chain multiple behaviors together

Each pipeline implements the trait:

```rust
#[async_trait]
pub trait IRequestPipeline: Send + Sync {
    async fn handle(
        &self,
        request: Box<dyn Any + Send + Sync>,
        next: Arc<dyn IErasedRequestHandler>,
    ) -> Result<Box<dyn Any + Send + Sync>, Box<dyn Error>>;
}
```

---

# 🛠 Step 1: Define a Pipeline

Here’s an example of a simple logging pipeline:

```rust
use async_trait::async_trait;
use std::any::Any;
use std::error::Error;
use std::sync::Arc;
use std::time::Instant;
use bus::core::contracts::{IRequestPipeline, IErasedRequestHandler};

#[derive(Default)]
pub struct LoggingPipeline;

#[async_trait]
#[bus::registry::BusRequestPipeline]
impl IRequestPipeline for LoggingPipeline {
    async fn handle(
        &self,
        request: Box<dyn Any + Send + Sync>,
        next: Arc<dyn IErasedRequestHandler>,
        #[cfg(feature = "cancellation-token")]
        cancellation_token: tokio_util::sync::CancellationToken,
    ) -> Result<Box<dyn Any + Send + Sync>, Box<dyn Error>> {
        let start = Instant::now();

        let result = next
            .handle(
                request,
                #[cfg(feature = "cancellation-token")]
                cancellation_token,
            )
            .await;

        let elapsed = start.elapsed();
        log::info!("🧭 Request handler executed in {:?}", elapsed);

        result
    }
}
```

# 🧩 Step 2: Register the Pipeline

You can register the pipeline `manually`:

```rust
use bus::core::registry::request_pipeline;

request_pipeline(|| Arc::new(LoggingPipeline::default()));
```

Or automatically using the procedural `macro`:

```rust
#[derive(Default)]
#[bus::registry::BusRequestPipeline]
pub struct LoggingPipeline;

#[async_trait]
impl IRequestPipeline for LoggingPipeline {
    // same as above
}
```

The macro generates a `#[ctor::ctor]` function that registers the pipeline at startup.

---

# 🧪 Example: Request + Handler

```rust
pub struct CreateUserCommand {
    pub username: String,
    pub email: String,
}

impl IRequest<(), AppError> for CreateUserCommand {}

#[derive(Default)]
pub struct CreateUserHandler;

#[async_trait]
#[bus::registry::BusRequestHandler]
impl IRequestHandler<CreateUserCommand, (), AppError> for CreateUserHandler {
    async fn handle_async(&self, command: CreateUserCommand) -> Result<(), AppError> {
        println!("Creating user: {}", command.username);
        Ok(())
    }
}
```

# 🚀 Sending a Request

```rust
bus::send(CreateUserCommand {
    username: "alice".into(),
    email: "alice@example.com".into(),
})
.await
.unwrap();
```


# 🧷 Optional: Graceful Shutdown with `cancellation-token`

If your application enables the `cancellation-token` feature in `bus`, each pipeline and handler will receive a `tokio_util::sync::CancellationToken` that can be used to gracefully abort long-running operations.

## ✅ Enabling the feature

In your `Cargo.toml:`

```toml
[dependencies.bus]
version = "..."
features = ["cancellation-token"]
```

## 🧠 How it works

When the feature is enabled:

* The `handle` method of `IRequestPipeline` and `IRequestHandler` receives an extra argument:

```rust
#[async_trait]
pub trait IRequestPipeline {
    async fn handle(
        &self,
        request: Box<dyn Any + Send + Sync>,
        next: Arc<dyn IErasedRequestHandler>,
        cancellation_token: CancellationToken,
    ) -> Result<Box<dyn Any + Send + Sync>, Box<dyn Error>>;
}
```

* You can use this token to:
  * Cancel HTTP requests
  * Abort retries
  * Exit loops early
  * Respect shutdown signals

---

# 🧼 Summary


| Step      | What to do   |
|:----------|:-------------|
| 1️⃣	    | Implement IRequestPipeline for your struct  |
| 2️⃣	    | Register it using request_pipeline(...) or #[BusRequestPipeline]    |
| 3️⃣	    | Use it to wrap request handlers with logging, validation, etc.    |
| 4️⃣	    | Pipelines are executed in reverse registration order    |
| 5️⃣	    | Pipelines are automatically applied to all request handlers    |


---

## 🧠 Best Practices

* [x] Keep pipelines focused and composable
* [x] Use #[derive(Default)] to simplify macro registration
* [x] Use feature flags like logging or metrics to toggle behavior
* [x] Avoid panics — always return Result
* [x] Use #[cfg(feature = "cancellation-token")] to support graceful shutdown