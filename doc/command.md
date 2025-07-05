# 🧭 Command Handling with `bus`

### This guide explains how to define, register, and execute Commands using the `bus` framework.

---

# ✅ What is a Command?

### A Command represents an intention to perform an action that changes system state. It does not return a value — only success or failure.

In `bus`, a Command is modeled as a type that implements:

```rust
impl IRequest<(), AppError> for MyCommand {}
```

### 🛠 Step 1: Define the Command

```rust
use bus::core::contracts::IRequest;
use app::domain::common::errors::AppError;

pub struct CreateUserCommand {
    pub username: String,
    pub email: String,
}

// Command returns nothing on success
impl IRequest<(), AppError> for CreateUserCommand {}
```

### 🧠 Step 2: Implement the Command Handler

```rust
use bus::core::contracts::IRequestHandler;
use async_trait::async_trait;

pub struct CreateUserHandler;

#[async_trait]
impl IRequestHandler<CreateUserCommand, (), AppError> for CreateUserHandler {
    async fn handle_async(&self, command: CreateUserCommand) -> Result<(), AppError> {
        println!("Creating user: {}", command.username);
        // Perform DB insert, send email, etc.
        Ok(())
    }
}
```

### 🧩 Step 3: Register the Command Handler

#### You can register the handler using either:

### Option A: With a `Default` derive `#[derive(Default)]`

```rust
#[derive(Default)]
pub struct CreateUserHandler;

#[async_trait]
#[bus::registry::BusRequestHandler]
impl IRequestHandler<CreateUserCommand, (), AppError> for CreateUserHandler {
    async fn handle_async(&self, command: CreateUserCommand) -> Result<(), AppError> {
        Ok(())
    }
}
```

### Option B: With a custom async `Factory`

```rust
use bus::core::factory::RequestProvidesFactory;

pub struct CreateUserHandler {
    pub config: String,
}

#[async_trait]
impl RequestProvidesFactory<CreateUserHandler, CreateUserCommand, (), AppError>
for CreateUserHandler
{
    async fn factory() -> Result<Self, AppError> {
        let config = std::env::var("CONFIG").map_err(AppError::from)?;
        Ok(Self { config })
    }
}

#[async_trait]
#[bus::registry::BusRequestHandler(factory)]
impl IRequestHandler<CreateUserCommand, (), AppError> for CreateUserHandler {
    async fn handle_async(&self, command: CreateUserCommand) -> Result<(), AppError> {
        Ok(())
    }
}
```

The macro generates a `#[ctor::ctor]` function that registers the pipeline at startup.


### 🚀 Step 4: Send the Command

```rust
bus::send(CreateUserCommand {
    username: "alice".into(),
    email: "alice@example.com".into(),
}).await?;

println!("User created successfully!");
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

### 🛡 Optional: Cancellation Support

#### If you enable the `cancellation-token` feature in your `Cargo.toml`:

```toml
[dependencies.bus]
version = "..."
features = ["cancellation-token"]
```

#### Then your handler can optionally accept a cancellation token:

```rust
#[async_trait]
impl IRequestHandler<CreateUserCommand, (), AppError> for CreateUserHandler {
    async fn handle_async(
        &self,
        command: CreateUserCommand,
        cancellation_token: tokio_util::sync::CancellationToken,
    ) -> Result<(), AppError> {
        if cancellation_token.is_cancelled() {
            return Err(AppError::cancelled());
        }

        // Proceed with command
        Ok(())
    }
}
```

#### This allows graceful shutdown or timeout-aware command execution.

---

### 🧼 Summary

| Step     | What to do                                    |
|:---------|:----------------------------------------------|
| Error    | Your error type must implement AnyError       |
| Define   | impl IRequest<(), AppError> for your command  |
| Handle   | impl IRequestHandler<_, (), AppError>         |
| Register | Use #[bus::registry::BusRequestHandler(...)]       |
| Execute  | bus::send(command).await                      |
| Optional | Enable "cancellation-token" for graceful exit |

