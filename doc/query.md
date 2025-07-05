# 🔎 Query Handling with `bus`

### This guide explains how to define, handle, and execute Queries using the `bus` framework.

---

# ✅ What is a Query?

### A Query represents a request to retrieve data without modifying system state — similar to an HTTP GET request. It returns a value on success.

In `bus`, a Query is modeled as a type that implements:


```rust
impl IRequest<UserResponse, AppError> for GetUserByEmailQuery {}
```

### 🛠 Step 1: Define the Query

```rust
use bus::core::contracts::IRequest;
use app::domain::common::errors::AppError;

pub struct GetUserByEmailQuery {
    pub email: String,
}

pub struct UserResponse {
    pub id: i32,
    pub email: String,
    pub username: String,
}

// Query returns a value on success
impl IRequest<UserResponse, AppError> for GetUserByEmailQuery {}
```

### 🧠 Step 2: Implement the Query Handler

```rust
use bus::core::contracts::IRequestHandler;
use async_trait::async_trait;

pub struct GetUserByEmailHandler;

#[async_trait]
impl IRequestHandler<GetUserByEmailQuery, UserResponse, AppError> for GetUserByEmailHandler {
    async fn handle_async(&self, query: GetUserByEmailQuery) -> Result<UserResponse, AppError> {
        // Simulate DB lookup
        Ok(UserResponse {
            id: 1,
            email: query.email.clone(),
            username: "alice".to_string(),
        })
    }
}
```

### 🧩 Step 3: Register the Query Handler

#### You can register the Query handler using either:

### Option A: With a `Default` derive `#[derive(Default)]`

```rust
#[derive(Default)]
pub struct GetUserByEmailHandler;

#[async_trait]
#[bus::registry::BusRequestHandler]
impl IRequestHandler<GetUserByEmailQuery, UserResponse, AppError> for GetUserByEmailHandler {
    async fn handle_async(&self, query: GetUserByEmailQuery) -> Result<UserResponse, AppError> {
        Ok(UserResponse {
            id: 1,
            email: query.email.clone(),
            username: "default_user".to_string(),
        })
    }
}
```

### Option B: With a custom async `Factory`

```rust
use bus::core::factory::RequestProvidesFactory;

pub struct GetUserByEmailHandler {
    pub db_url: String,
}

#[async_trait]
impl RequestProvidesFactory<GetUserByEmailHandler, GetUserByEmailQuery, UserResponse, AppError>
for GetUserByEmailHandler
{
    async fn factory() -> Result<Self, AppError> {
        let db_url = std::env::var("DATABASE_URL").map_err(AppError::from)?;
        Ok(Self { db_url })
    }
}

#[async_trait]
#[bus::registry::BusRequestHandler(factory)]
impl IRequestHandler<GetUserByEmailQuery, UserResponse, AppError> for GetUserByEmailHandler {
    async fn handle_async(&self, query: GetUserByEmailQuery) -> Result<UserResponse, AppError> {
        Ok(UserResponse {
            id: 1,
            email: query.email.clone(),
            username: "from_db".to_string(),
        })
    }
}
```

The macro generates a `#[ctor::ctor]` function that registers the pipeline at startup.

### 🚀 Step 4: Execute the Query

```rust
let user = bus::send(GetUserByEmailQuery {
    email: "alice@example.com".into(),
}).await?;

println!("User: {} <{}>", user.username, user.email);

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
impl IRequestHandler<GetUserByEmailQuery, UserResponse, AppError> for GetUserByEmailHandler {
    async fn handle_async(
        &self,
        query: GetUserByEmailQuery,
        cancellation_token: tokio_util::sync::CancellationToken,
    ) -> Result<UserResponse, AppError> {
        if cancellation_token.is_cancelled() {
            return Err(AppError::Server("Cancelled".into()));
        }

        Ok(UserResponse {
            id: 1,
            email: query.email.clone(),
            username: "cancel-safe".to_string(),
        })
    }
}
```

#### This allows graceful shutdown or timeout-aware command execution.

---

### 🧼 Summary

| Step     | What to do                                         |
|:---------|:---------------------------------------------------|
| Error    | Your error type must implement AnyError            |
| Define   | impl IRequest<(), AppError> for your command       |
| Handle   | impl IRequestHandler<_, (), AppError>              |
| Register | Use #[bus::registry::BusRequestHandler(...)]       |
| Execute  | bus::send(query).await                             |
| Optional | Enable "cancellation-token" for graceful exit      |

