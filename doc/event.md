# 🧠 In-Memory Events

#### This mode allows you to use rust_bus as a high-performance internal Mediator. It is designed for decoupling business logic within a single process where persistent background processing (database-backed) is not required.


---

## 📦 Installation

#### To use the Bus strictly for in-memory operations, add the package with minimal features to your Cargo.toml:

```toml
[dependencies]
# Basic version (no context, no database)
rust_bus = { version = "3" }

# With Context support
rust_bus = { version = "3", features = ["context"] }

# With DataBase support
rust_bus = { version = "3", features = [
   # ONE OF
   "sea-orm-postgres",
   "sea-orm-mysql",
   "sqlx-postgres",
   "sqlx-mysql",
] }
```

---

## 🚀🚀 No Context + No DataBase

```rust
use rust_bus::contracts::meta::BusMetadata;
use rust_bus::{bus, BusEvent, BusEventHandler};

#[BusEvent]
pub struct UserRegisteredEvent {
    pub email: String,
}

#[BusEventHandler]
async fn on_user_created(
    event: &UserRegisteredEvent,
    meta: &BusMetadata,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("User registered: {}", event.email);
    Ok(())
}

#[tokio::main]
async fn main() {
    // Automatically registers all handlers and events in the project
    rust_bus::init().await.unwrap();
    
    let event = UserRegisteredEvent{ 
        email: "example@gmail.com".to_string() 
    };
    let meta = BusMetadata::default();
    
    bus::event(event, &meta).await.unwrap();
}
```

---

## 🚀🐘🐬 No Context + SeaORM

```rust
use rust_bus::contracts::meta::BusMetadata;
use rust_bus::{bus, BusEvent, BusEventHandler};
use rust_bus::workers::configuration::BusQueueConfigurationBuilder;

#[BusEvent]
pub struct UserRegisteredEvent {
    pub email: String,
}

#[BusEventHandler]
async fn on_user_created(
    txn: &sea_orm::DatabaseTransaction,
    event: &UserRegisteredEvent,
    meta: &BusMetadata,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("User registered: {}", event.email);
    Ok(())
}

#[tokio::main]
async fn main() {
    let pool = sea_orm::ConnectOptions::new("database_url").clone();
    
    // Automatically registers all handlers and events in the project
    let bus_cfg = BusQueueConfigurationBuilder::default().connection(pool);
    rust_bus::init(bus_cfg).await.unwrap();

    let txn = pool.begin().await.unwrap();

    let event = UserRegisteredEvent{ 
        email: "example@gmail.com".to_string() 
    };
    let meta = BusMetadata::default();
    
    bus::event(&txn, event, &meta).await.unwrap();

    txn.commit().await.unwrap();
}
```


---

## 🚀🐘 No Context + SQLX Postgres

```rust
use rust_bus::contracts::meta::BusMetadata;
use rust_bus::{bus, BusEvent, BusEventHandler};
use rust_bus::workers::configuration::BusQueueConfigurationBuilder;

#[BusEvent]
pub struct UserRegisteredEvent {
    pub email: String,
}

#[BusEventHandler]
async fn on_user_created(
    txn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    event: &UserRegisteredEvent,
    meta: &BusMetadata,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("User registered: {}", event.email);
    Ok(())
}

#[tokio::main]
async fn main() {
    let pool = sqlx::postgres::PgPoolOptions::new().connect("db_url").await.unwrap();
    
    // Automatically registers all handlers and events in the project
    let bus_cfg = BusQueueConfigurationBuilder::default().connection(pool);
    rust_bus::init(bus_cfg).await.unwrap();

    let mut txn = pool.begin().await.unwrap();

    let event = UserRegisteredEvent{ 
        email: "example@gmail.com".to_string() 
    };
    let meta = BusMetadata::default();
    
    bus::event(&mut txn, event, &meta).await.unwrap();

    txn.commit().await.unwrap();
}
```


---

## 🚀🐬 No Context + SQLX Mysql

```rust
use rust_bus::contracts::meta::BusMetadata;
use rust_bus::{bus, BusEvent, BusEventHandler};
use rust_bus::workers::configuration::BusQueueConfigurationBuilder;

#[BusEvent]
pub struct UserRegisteredEvent {
    pub email: String,
}

#[BusEventHandler]
async fn on_user_created(
    txn: &mut sqlx::Transaction<'_, sqlx::MySql>,
    event: &UserRegisteredEvent,
    meta: &BusMetadata,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("User registered: {}", event.email);
    Ok(())
}

#[tokio::main]
async fn main() {
    let pool = sqlx::mysql::MySqlPoolOptions::new().connect("db_url").await.unwrap();
    
    // Automatically registers all handlers and events in the project
    let bus_cfg = BusQueueConfigurationBuilder::default().connection(pool);
    rust_bus::init(bus_cfg).await.unwrap();

    let mut txn = pool.begin().await.unwrap();

    let event = UserRegisteredEvent{ 
        email: "example@gmail.com".to_string() 
    };
    let meta = BusMetadata::default();
    
    bus::event(&mut txn, event, &meta).await.unwrap();

    txn.commit().await.unwrap();
}
```

---

## 🧬🚀 Context + No DataBase

```rust
use rust_bus::contracts::meta::BusMetadata;
use rust_bus::{bus, BusEvent, BusEventHandler, ExampleBusContext};
use rust_bus::workers::configuration::BusQueueConfigurationBuilder;

#[BusEvent]
pub struct UserRegisteredEvent {
    pub email: String,
}

#[BusEventHandler]
async fn on_user_created(
    ctx: &ExampleBusContext,
    event: &UserRegisteredEvent,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("User registered: {}", event.email);
    Ok(())
}

#[tokio::main]
async fn main() {
    // Automatically registers all handlers and events in the project
    rust_bus::init().await.unwrap();

    let ctx = ExampleBusContext::new(BusMetadata::default());
    
    let event = UserRegisteredEvent{ 
        email: "example@gmail.com".to_string() 
    };
    
    bus::event(&ctx, event).await.unwrap();
}
```


---

## 🧬🐘🐬 Context + SeaORM

```rust
use rust_bus::contracts::meta::BusMetadata;
use rust_bus::{bus, BusEvent, BusEventHandler, ExampleBusContext};
use rust_bus::workers::configuration::BusQueueConfigurationBuilder;

#[BusEvent]
pub struct UserRegisteredEvent {
    pub email: String,
}

#[BusEventHandler]
async fn on_user_created(
    ctx: &ExampleBusContext<'_>,
    event: &UserRegisteredEvent,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("User registered: {}", event.email);
    Ok(())
}

#[tokio::main]
async fn main() {
    let pool = sea_orm::ConnectOptions::new("database_url").clone();
    // Automatically registers all handlers and events in the project
    let bus_cfg = BusQueueConfigurationBuilder::default().connection(pool);
    rust_bus::init(bus_cfg).await.unwrap();

    let mut txn = pool.begin().await.unwrap();
    let ctx = ExampleBusContext::new(&txn, BusMetadata::default());

    let event = UserRegisteredEvent{ 
        email: "example@gmail.com".to_string() 
    };
    
    bus::event(&ctx, event).await.unwrap();

    txn.commit().await.unwrap();
}
```


---

## 🧬🐘🐬 Context + SQLX

```rust
use rust_bus::contracts::meta::BusMetadata;
use rust_bus::{bus, BusEvent, BusEventHandler, ExampleBusContext};
use rust_bus::workers::configuration::BusQueueConfigurationBuilder;

#[BusEvent]
pub struct UserRegisteredEvent {
    pub email: String,
}

#[BusEventHandler]
async fn on_user_created(
    ctx: &mut ExampleBusContext<'_, '_>,
    event: &UserRegisteredEvent,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("User registered: {}", event.email);
    Ok(())
}

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

    let bus_cfg = BusQueueConfigurationBuilder::default().connection(pool);
    
    // Automatically registers all handlers and events in the project
    rust_bus::init(bus_cfg).await.unwrap();

    let mut txn = pool.begin().await.unwrap();
    let mut ctx = ExampleBusContext::new(&mut txn, BusMetadata::default());

    let event = UserRegisteredEvent{ 
        email: "example@gmail.com".to_string() 
    };
    
    bus::event(&mut ctx, event).await.unwrap();

    txn.commit().await.unwrap();
}
```
