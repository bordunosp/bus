# 🛰️ Database-backed Event Processing with `bus` + `SeaORM`

### This guide explains how to define, publish, and process events asynchronously using the `bus` framework with a database-backed queue powered by `SeaORM` and either `MySQL` or `PostgreSQL`.

---
# ✅ What is a Database-Backed Event?

### Unlike `In-memory` events, `Database-backed` events are:

* ✉️ Persisted in a table (e.g. bus_events)
* 🕒 Processed by background workers
* 🔁 Retryable with backoff logic
* 🔒 Optionally published within a database transaction


### This makes them ideal for:

1. [ ] Deferred side effects (e.g. sending emails)
2. [ ] Cross-service communication
3. [ ] Reliable background processing

---


# ⚙️ Installation
### In your `Cargo.toml`:
```toml
[dependencies.bus]
version = "..."
features = ["sea-orm-postgres"] # or "sea-orm-mysql", but not both
# Optional:
# features = ["json-payload", "logging"]
```

### ⚠️ Only one of `sea-orm-postgres` or `sea-orm-mysql` can be enabled at a time.

---

# 📦 Migrations
### All required database migrations (for creating `bus_events`, `bus_events_archive`, etc.) are included in the bus crate.

You can find them in the root directory of the `bus` package at:

```
bus/migration/
```
Use your preferred migration tool to apply them before running workers.

---

# 📬 Example: SendEmailEvent

### Let’s walk through a real-world example: sending a confirmation email after user registration.

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


### 🛠 Step 1: Define the Database Event

```rust
use bus::core::contracts::IEvent;
use serde::{Serialize, Deserialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct SendEmailEvent {
    pub to: String,
    pub subject: String,
    pub body: String,
}

impl IEvent<AppError> for SendEmailEvent {}
```

### 🧠 Step 2: Implement the Database Event Handler

```rust
use bus::core::contracts::{IEventHandlerDatabase, EventQueueSettings};
use async_trait::async_trait;

#[derive(Default)]
pub struct SendEmailHandler;

#[async_trait]
impl IEventHandlerDatabase<SendEmailEvent, AppError> for SendEmailHandler {
    fn settings() -> EventQueueSettings {
        EventQueueSettings {
            queue_name: "email".into(),
            max_retries: 3,
            execution_timeout: chrono::Duration::minutes(5),
        }
    }

    fn calculate_scheduled_at(current_retry: i32) -> chrono::Duration {
        match current_retry {
            1 => chrono::Duration::minutes(30),
            2 => chrono::Duration::minutes(60),
            _ => chrono::Duration::minutes(90),
        }
    }

    async fn handle_async(&self, event: SendEmailEvent) -> Result<(), AppError> {
        println!("📧 Sending email to {}: {}", event.to, event.subject);
        // call your email service here
        Ok(())
    }
}
```

### 🧩 Step 3: Register the Database Event Handler

#### You can register the Event handler using either:

### Option A: With a `Default` derive `#[derive(Default)]`

```rust
use bus::core::factory::EventDatabaseProvidesFactory;

#[derive(Default)]
pub struct IEventHandlerDatabase;

#[async_trait]
#[use bus::registry::BusEventDatabaseHandler]
impl IEventHandlerDatabase<UserLoggedInEvent, AppError> for IEventHandlerDatabase {
    async fn handle_async(&self, event: SendEmailEvent) -> Result<(), AppError> {
        println!("📧 Sending email to {}: {}", event.to, event.subject);
        // call your email service here
        Ok(())
    }
}
```

### Option B: With a custom async `Factory`

```rust
use bus::core::factory::EventDatabaseProvidesFactory;

pub struct IEventHandlerDatabase {
    from: String 
}

#[async_trait]
impl EventDatabaseProvidesFactory<SendEmailHandler, SendEmailEvent, AppError> for SendEmailHandler {
    async fn factory() -> Result<Self, AppError> {
        Ok(Self{
            from: "from@email.com".to_string()
        })
    }
}

#[async_trait]
#[use bus::registry::BusEventDatabaseHandler(factory)]
impl IEventHandlerDatabase<UserLoggedInEvent, AppError> for IEventHandlerDatabase {
    async fn handle_async(&self, event: SendEmailEvent) -> Result<(), AppError> {
        println!("📧 Sending email to {}: {}. From: {}", event.to, event.subject, self.from);
        // call your email service here
        Ok(())
    }
}
```

The macro generates a `#[ctor::ctor]` function that registers the pipeline at startup.

### 🚀 Step 4: You can Emit the Database Event using either: 

### 🔹 Publish Normally

```rust
bus::publish(SendEmailEvent {
    to: "user@example.com".into(),
    subject: "Welcome!".into(),
    body: "Thanks for signing up.".into(),
}).await?;
```

### 🔸 Publish Within a Transaction

```rust
let txn = db.begin().await?;

bus::publish_txn(
    txn,
    SendEmailEvent {
        to: "user@example.com".into(),
        subject: "Welcome!".into(),
        body: "Thanks for signing up.".into(),
    },
).await?;
```

This ensures the email is only queued if the transaction commits.

---

### 🧰 Step 5: Initialize Queue Configuration

```rust
use bus::core::features::sea_orm::core::{DatabaseConnection, DatabaseQueueConfiguration};
use bus::core::features::sea_orm::dto::{DatabaseConnectionDto, DatabaseQueueConfigurationDto};

#[tokio::main]
async fn main() {
    bus::core::initialization::sea_orm(
        DatabaseQueueProcessorConfiguration::new(
            DatabaseConnection::new(DatabaseConnectionDto {
                sea_orm_connect_options: sea_orm::ConnectOptions::new(
                    "postgres://user_name:user_password@localhost:5432/app_db_name"
                )
                    .max_connections(100)
                    .min_connections(5)
                    .clone(),
                table_name: String::from("bus_events"),
                table_name_archive: String::from("bus_events_archive"),
            })?,
            vec![
                DatabaseQueueConfiguration::new(DatabaseQueueConfigurationDto {
                    queue_name: String::from("high"),
                    workers: 10,
                    batch_size: 10,
                    sleep_interval: std::time::Duration::from_secs(5),
                    archive_type: ArchiveType::All,
                }).expect("Failed to initialize high queue"),

                DatabaseQueueConfiguration::new(DatabaseQueueConfigurationDto {
                    queue_name: String::from("low"),
                    workers: 5,
                    batch_size: 5,
                    sleep_interval: std::time::Duration::from_secs(60),
                    archive_type: ArchiveType::None,
                }).expect("Failed to initialize low queue"),

                DatabaseQueueConfiguration::new(DatabaseQueueConfigurationDto {
                    queue_name: String::from("email"),
                    workers: 5,
                    batch_size: 5,
                    sleep_interval: std::time::Duration::from_secs(60),
                    archive_type: ArchiveType::Failed,
                }).expect("Failed to initialize low email"),
            ],
        )
    ).await.expect("Failed Sea-Orm database connection");
}
```

### 🧵 Step 6: Start the Worker Controller

```rust
use bus::core::features::sea_orm::workers::DatabaseWorkerController;

#[tokio::main]
async fn main() {
    let controller = DatabaseWorkerController::new();
    controller.start_workers().await?;

    // Graceful shutdown
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            log::info!("🛑 Ctrl+C received, shutting down...");
        }
    }

    controller.shutdown();
    controller.await_termination_with_timeout(std::time::Duration::from_secs(60)).await;
}
```


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

## 🧩 Optional Features


| Feature     | Description                                    |
|:---------|:----------------------------------------------|
| json-payload    | Stores a JSON representation of the event alongside the binary payload (for debugging/logging)       |
| logging    | Enables detailed logs for event processing, retries, and failures       |


## 🧼 Summary

| Step      | What to do                                                                                                                |
|:----------|:--------------------------------------------------------------------------------------------------------------------------|
| Define    | Create a struct like SendEmailEvent that implements IEvent<TError> and derives Clone, Serialize, Deserialize              |
| Handle    | Implement IEventHandlerDatabase<Event, Error> with handle_async(...), settings(), and calculate_scheduled_at(...)         |
| Register  | Use #[BusEventDatabaseHandler] or #[BusEventDatabaseHandler(factory)] to register the handler with optional factory logic |
| Emit      | Use bus::publish(...) to enqueue events, or bus::publish_txn(...) to publish within a transaction                         |
| Configure | Initialize queues using DatabaseQueueProcessorConfiguration with queue-specific settings                                                                                                                       |
| Run       | Start background workers using DatabaseWorkerController to process queued events                                                                                                                    |
| Migrate   | Apply database migrations from bus/migration/ before running the system                                                                                                                    |
| Extend    | Optionally enable features like json-payload, logging, or cancellation-token for enhanced behavior                        |
| Errors    | Ensure your error type implements AnyError (e.g. AppError) for compatibility with the bus framework                       |
