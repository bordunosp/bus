# 🧩 Database Event Pipelines in `bus`

This guide explains how to define and register middleware-style pipelines for database-backed events in the `bus` framework. 

Pipelines allow you to intercept, wrap, and extend the behavior of database event handlers — for logging, metrics, tracing, error handling, and more.

---

## ✅ What is a Pipeline?
### A pipeline is a composable unit that wraps around an event handler and can:

* ⏱ Measure execution time
* 🧾 Log inputs and outputs
* 🧪 Catch and transform errors
* 📊 Emit metrics or traces
* 🔁 Chain multiple behaviors together

Each pipeline implements the trait:

```rust
#[async_trait]
pub trait IEventDatabasePipeline: Send + Sync {
    async fn handle(
        &self,
        request: Box<dyn Any + Send + Sync>,
        next: Arc<dyn IErasedEventHandlerDatabase>,
    ) -> Result<(), Box<dyn Error + Send + Sync>>;
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
use bus::core::contracts::{IEventDatabasePipeline, IErasedEventHandlerDatabase};

#[derive(Default)]
pub struct LoggingPipeline;

#[async_trait]
impl IEventDatabasePipeline for LoggingPipeline {
    async fn handle(
        &self,
        request: Box<dyn Any + Send + Sync>,
        next: Arc<dyn IErasedEventHandlerDatabase>,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let start = Instant::now();
        let result = next.handle(request).await;
        let elapsed = start.elapsed();

        #[cfg(feature = "logging")]
        log::info!("🧭 Handler executed in {:?}", elapsed);

        result
    }
}
```

# 🧩 Step 2: Register the Database Pipeline

You can register the pipeline `manually`:

```rust
use bus::core::registry::event_database_pipeline;

event_database_pipeline(|| Arc::new(LoggingPipeline::default()));
```

Or `automatically` using the procedural `macro`:

```rust
#[derive(Default)]
#[bus::registry::BusEventDatabasePipeline]
pub struct LoggingPipeline;

#[async_trait]
impl IEventDatabasePipeline for LoggingPipeline {
    // same as above
}
```

The macro generates a `#[ctor::ctor]` function that registers the pipeline at startup.


# 🔁 Step 3: Pipeline Execution Order

### All registered pipelines are composed into a stack. The last registered pipeline wraps the handler first.

#### For example, if you register:

```rust
event_database_pipeline(|| Arc::new(MetricsPipeline));
event_database_pipeline(|| Arc::new(LoggingPipeline));
```

#### Then the execution order will be:

```
MetricsPipeline → LoggingPipeline → ActualHandler
```

Each pipeline receives the event and a reference to the next handler in the chain.


---

# 🧪 Example: Metrics Pipeline

```rust
#[derive(Default)]
#[bus::registry::BusEventDatabasePipeline]
pub struct MetricsPipeline;

#[async_trait]
impl IEventDatabasePipeline for MetricsPipeline {
    async fn handle(
        &self,
        request: Box<dyn Any + Send + Sync>,
        next: Arc<dyn IErasedEventHandlerDatabase>,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let result = next.handle(request).await;

        match &result {
            Ok(_) => log::info!("✅ Event processed successfully"),
            Err(e) => log::warn!("⚠️ Event failed: {}", e),
        }

        result
    }
}
```

---

## 🧼 Summary

| Step      | What to do   |
|:----------|:-------------|
| 1️⃣	    | Implement IEventDatabasePipeline for your struct  |
| 2️⃣	    | Register it using event_database_pipeline(...) or #[BusEventDatabasePipeline]    |
| 3️⃣	    | Use it to wrap event handlers with logging, metrics, tracing, etc.    |
| 4️⃣	    | Pipelines are executed in reverse registration order    |
| 5️⃣	    | Pipelines are automatically applied to all database event handlers    |


---

## 🧠 Best Practices

1. [ ] Keep pipelines focused and composable
2. [ ] Use `#[derive(Default)]` to simplify macro registration
3. [ ] Use feature flags like logging or metrics to toggle behavior
4. [ ] Avoid panics — always return Result

