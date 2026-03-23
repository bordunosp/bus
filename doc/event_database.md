# 🗃️ Database-Backed Events (Persistent Jobs)

#### Persistent jobs provide transactional integrity. A job is only enqueued if your database transaction is committed. If a job fails, it will be retried with an exponential backoff.

---

## 📦 Installation

#### To use the Bus strictly for Persistent operations, add the package with minimal features to your Cargo.toml:

```toml
[dependencies]
# With DataBase support
rust_bus = { version = "3", features = [
    # ONE OF
    "sea-orm-postgres",
    "sea-orm-mysql",
    "sqlx-postgres",
    "sqlx-mysql",
] }

# With Context support
rust_bus = { version = "3", features = ["context"] }
```

---

# Configuration

```rust
use rust_bus::workers::configuration::{BusQueueConfigurationBuilder, QueueConfigBuilder};
use rust_bus::workers::queue::worker;
use std::time::Duration;

#[tokio::main]
async fn main() {
    #[cfg(feature = "sqlx-postgres")]
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect("db_url")
        .await
        .unwrap();

    #[cfg(feature = "sqlx-mysql")]
    let pool = sqlx::mysql::MySqlPoolOptions::new()
        .max_connections(10)
        .connect("db_url")
        .await
        .unwrap();

    #[cfg(feature = "sea-orm-postgres")]
    let pool = sea_orm::ConnectOptions::new("database_url").clone();

    #[cfg(feature = "sea-orm-mysql")]
    let pool = sea_orm::ConnectOptions::new("database_url").clone();

    rust_bus::init(
        BusQueueConfigurationBuilder::default()
            .srv_name("Any custom server Name")
            .connection(pool)
            .add_queue(
                "high",
                QueueConfigBuilder::default()
                    .workers(10)
                    .max_attempts(5)
                    .empty_queue_delay(Duration::from_secs(5))
                    .execution_timeout(chrono::Duration::minutes(10)),
            )
            .add_queue(
                "low",
                QueueConfigBuilder::default()
                    .workers(5)
                    .max_attempts(3)
                    .empty_queue_delay(Duration::from_secs(30))
                    .execution_timeout(chrono::Duration::minutes(10)),
            )
            .add_queue(
                "emails",
                QueueConfigBuilder::default()
                    .workers(3)
                    .max_attempts(3)
                    .empty_queue_delay(Duration::from_secs(60))
                    .execution_timeout(chrono::Duration::minutes(3)),
            ),
    ).await.unwrap();

    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(());

    log::info!("🚀 System started. Press Ctrl+C to stop.");
    worker::start(shutdown_rx.clone()).await?;

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            log::info!("🛑 Ctrl+C detected! Notifying workers...");
        }
    }

    let _ = shutdown_tx.send(());

    log::info!("⏳ Waiting for active jobs to finalize...");
    if worker::wait_or_shutdown(&mut shutdown_rx, Duration::from_secs(20)).await {
        log::info!("✅ All workers confirmed shutdown.");
    } else {
        log::warn!("⚠️ Shutdown timeout reached. Some tasks might be killed.");
    }
}
```

---

#### Use the BusEventHandlerDatabase macro. This handler will receive a DatabaseConnection and can be configured with specific queue settings.

# 🚀🐘🐬 Minimal Example Configuration

```rust
use rust_bus::contracts::meta::BusMetadata;
use rust_bus::{BusEvent, BusEventHandlerDatabase};


#[BusEvent]
pub struct UserRegisteredEvent {
    pub email: String,
}

#[derive(Default)]
pub struct SendEmailHandler;

#[BusEventHandlerDatabase]
impl IEventHandlerDatabase<UserRegisteredEvent> for SendEmailHandler {
    const QUEUE: &'static str = "email";

    async fn handle(
        &self,

        #[cfg(feature = "sqlx-postgres")]
        db: &sqlx::PgPool,

        #[cfg(feature = "sqlx-mysql")]
        db: &sqlx::MySqlPool,

        #[cfg(feature = "sea-orm-postgres")]
        db: &sea_orm::DatabaseConnection,

        #[cfg(feature = "sea-orm-mysql")]
        db: &sea_orm::DatabaseConnection,

        event: &UserRegisteredEvent,
        metadata: &BusMetadata,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }
}
```

---

# 🚀🐘🐬 Full Example Configuration

```rust
use rust_bus::contracts::meta::BusMetadata;
use rust_bus::{BusEvent, BusEventHandlerDatabase};
use rust_bus::contracts::enums::{Field, Period, Replace, State, ScheduleIn, AvailableField, RetryableField};


#[BusEvent]
pub struct UserRegisteredEvent {
    pub email: String,
}

#[derive(Default)]
pub struct SendEmailHandler;

#[BusEventHandlerDatabase]
impl IEventHandlerDatabase<UserRegisteredEvent> for SendEmailHandler {
    const QUEUE: &'static str = "email";
    const PRIORITY: u32 = 10;
    const MAX_ATTEMPTS: Option<u32> = Some(10);
    const EXECUTION_TIMEOUT: Option<chrono::Duration> = Some(chrono::Duration::minutes(30));
    const SCHEDULE_IN: ScheduleIn = ScheduleIn::Duration(std::time::Duration::from_hours(1));

    const TAGS: &'static [&'static str] = &[
        "emails",
        "users",
        "any_for_other_statistics"
    ];

    const UNIQUE: Option<Unique> = Some(Unique {
        period: Period::Infinite,
        fields: &[Field::Tags, Field::Meta],
        keys: &["user_id", "user_email"],
        states: &[State::Available, State::Retryable],
        replace: &[
            Replace::Available(&[
                AvailableField::Event,
                AvailableField::Priority,
                AvailableField::MaxAttempts,
            ]),
            Replace::Retryable(&[
                RetryableField::Event,
                RetryableField::Priority,
                RetryableField::MaxAttempts,
            ]),
        ],
    });

    fn next_attempt_at(current_attempt: u32) -> chrono::Duration {
        match current_attempt {
            1 => chrono::Duration::minutes(15),
            2 => chrono::Duration::minutes(30),
            _ => chrono::Duration::minutes(60),
        }
    }

    async fn handle(
        &self,

        #[cfg(feature = "sqlx-postgres")]
        db: &sqlx::PgPool,

        #[cfg(feature = "sqlx-mysql")]
        db: &sqlx::MySqlPool,

        #[cfg(feature = "sea-orm-postgres")]
        db: &sea_orm::DatabaseConnection,

        #[cfg(feature = "sea-orm-mysql")]
        db: &sea_orm::DatabaseConnection,

        event: &UserRegisteredEvent,
        metadata: &BusMetadata,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }
}
```