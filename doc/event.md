# 📡 Event Handling with `bus`

### This guide explains how to define, handle, and emit `In-memory events` using the bus framework.

---
# ✅ What is an Event?

An Event represents something that has **already happened** in the system — like **UserLoggedInEvent**.

In `bus`, events are executed **In-memory**: All registered handlers are awaited and must complete before bus::publish(...) returns.

This makes event handling:

* 🔄 Predictable
* 🧩 Composable in business logic
* 🧠 Suitable for side effects like logging, metrics, or in-memory updates

❗ This guide focuses on `In-memory events`. For deferred or outbox-style events, see the upcoming guide on **Database-backed events** event delivery.

---

### 🧱 Event Requirements
To define a valid event in `bus`, your type must:


| Trait / Bound     | Purpose                                    |
|:---------|:----------------------------------------------|
| Clone    | Your error type must implement AnyError       |
| Serialize + Deserialize | Enable "cancellation-token" for graceful exit |
| Send + Sync + 'static | Safe for async execution |
| IEvent<TError> | Marker trait for event registration |

---

### 🛠 Step 1: Define the Event

```rust
use bus::core::contracts::IEvent;
use app::domain::common::errors::AppError;
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct UserLoggedInEvent {
    pub user_id: i32,
    pub timestamp: DateTime<Utc>,
}

impl IEvent<AppError> for UserLoggedInEvent {}
```

### 🧠 Step 2: Implement the Event Handler

```rust
use bus::core::contracts::IEventHandler;
use async_trait::async_trait;

pub struct UpdateLastLoginHandler;

#[async_trait]
impl IEventHandler<UserLoggedInEvent, AppError> for UpdateLastLoginHandler {
    async fn handle_async(&self, event: UserLoggedInEvent) -> Result<(), AppError> {
        println!("🕒 User {} logged in at {}", event.user_id, event.timestamp);
        Ok(())
    }
}
```

### 🧩 Step 3: Register the Event Handler

#### You can register the Event handler using either:

### Option A: With a `Default` derive `#[derive(Default)]`

```rust
#[derive(Default)]
pub struct UpdateLastLoginHandler;

#[async_trait]
#[bus::registry::BusEventHandler]
impl IEventHandler<UserLoggedInEvent, AppError> for UpdateLastLoginHandler {
    async fn handle_async(&self, event: UserLoggedInEvent) -> Result<(), AppError> {
        println!("✅ Updated last login for user {}", event.user_id);
        Ok(())
    }
}
```

### Option B: With a custom async `Factory`

```rust
use bus::core::factory::EventProvidesFactory;

pub struct UpdateLastLoginHandler {
    pub source: String,
}

#[async_trait]
impl EventProvidesFactory<UpdateLastLoginHandler, UserLoggedInEvent, AppError>
for UpdateLastLoginHandler
{
    async fn factory() -> Result<Self, AppError> {
        Ok(Self {
            source: "auth-service".to_string(),
        })
    }
}

#[async_trait]
#[bus::registry::BusEventHandler(factory)]
impl IEventHandler<UserLoggedInEvent, AppError> for UpdateLastLoginHandler {
    async fn handle_async(&self, event: UserLoggedInEvent) -> Result<(), AppError> {
        println!("📍 [{}] login: {}", self.source, event.user_id);
        Ok(())
    }
}
```

The macro generates a `#[ctor::ctor]` function that registers the pipeline at startup.

### 🚀 Step 4: Emit the Event

```rust
use chrono::Utc;

bus::publish(UserLoggedInEvent {
    user_id: 42,
    timestamp: Utc::now(),
}).await?;
```
All registered handlers will be executed `In-memory` before this call returns

---

### ⚠️ Error Type Requirements
#### Your error type (e.g. AppError) must implement the AnyError trait:
#### Where AppError is `yours custom app error`!!!
```rust
use bus::core::contracts::BusError;
use std::error::Error;

pub trait AnyError: Error + From<BusError> + Send + Sync + 'static {}

impl<T> AnyError for T where 
    T: Error + From<BusError> + Send + Sync + 'static {}
```

#### Example of use in a real application
```rust
use fang::ToFangError;
use thiserror::Error;
use std::fmt::Debug;

#[derive(Debug, Error, ToFangError)]
pub enum AppError {
    #[error("Server Error: {0}")]
    Server(String),

    #[error("BusError: {0}")]
    Bus(#[from] bus::core::error_bus::BusError),
}
```

---

### 🔄 Optional: Transactional Event Publishing

#### If you're using a database backend like PostgreSQL or MySQL via sea-orm, you can publish events inside a transaction using publish_txn(...).
#### If you enable the `sea-orm` feature in your Cargo.toml:

### ✨ Example

```toml
[dependencies.bus]
version = "..."
features = ["sea-orm-postgres"]
features = ["sea-orm-mysql"]
```

```rust
let txn = db.begin().await?;

bus::publish_txn(
    txn,
    UserLoggedInEvent {
        user_id: 42,
        timestamp: Utc::now(),
    },
).await?;
```
This guarantees atomicity between event logic and DB changes.

---


### 🛡 Optional: Cancellation Support

#### If you enable the `cancellation-token` feature in your `Cargo.toml`:

```toml
[dependencies.bus]
version = "..."
features = ["cancellation-token"]
```

#### Then your handler can optionally accept a cancellation token:

```rust
use bus::core::contracts::IEventHandler;
use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

#[derive(Default)]
pub struct UpdateLastLoginHandler;

#[async_trait]
#[bus::registry::BusEventHandler]
impl IEventHandler<UserLoggedInEvent, AppError> for UpdateLastLoginHandler {
    async fn handle_async(
        &self,
        event: UserLoggedInEvent,
        cancellation_token: CancellationToken,
    ) -> Result<(), AppError> {
        if cancellation_token.is_cancelled() {
            return Err(AppError::Server("Login event cancelled".into()));
        }

        println!("🕒 User {} logged in at {}", event.user_id, event.timestamp);
        Ok(())
    }
}
```

#### This allows graceful shutdown or timeout-aware command execution.

---

### 🧼 Summary

| Step     | What to do                                    |
|:---------|:----------------------------------------------|
| Define    | Create a struct that implements IEvent<TError> and derives Clone, Serialize, Deserialize       |
| Handle | Implement IEventHandler<Event, Error> with async fn handle_async(...) |
| Register | Use #[BusEventHandler] or #[BusEventHandler(factory)] to register the handler |
| Emit | Call bus::publish(event).await to trigger all handlers `In-memory` |
| Txn | Use bus::publish_txn(...) to emit events within a database transaction |
| Cancel | Enable cancellation-token feature to support graceful cancellation |
| Errors | Ensure your error type implements AnyError (e.g. AppError) |

