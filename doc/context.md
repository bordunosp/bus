# 🌐 Context System:

#### Managing Runtime StateThe Context System in rust_bus is the backbone of its reliability and developer ergonomics. It acts as a bridge between your application's infrastructure (Database, Tracing) and your business logic (Event Handlers).

#### Instead of passing dozens of arguments to every function, you pass a single Context that carries everything needed for a successful execution.

---

# 🧭 Why Context Matters

In real-world applications, an event handler rarely works in a vacuum.
It usually **needs:Database Transactions**: To ensure that if a handler fails, everything is rolled **back.Metadata**:
To track **request_id** or **correlation_id** across **logs.Safety**: To ensure that immutable handlers cannot
accidentally modify state.
The **rust_bus** context system manages these concerns **automatically**, allowing you to **focus** on the **code** that
matters.

### 🛠 Basic UsageEvery handler receives a reference to a context.

You can use the built-in **ExampleBusContext** or define your **own.Dispatching** an **EventRust**. Prepare your
infrastructure

```rust
use rust_bus::*;

#[BusEventHandler]
async fn on_user_registered(
    ctx: &ExampleBusContext<'_>, // Read-only access
    event: &UserRegistered
) -> BusResult {
    println!("Processing user {} with Request ID: {:?}", event.id, ctx.metadata().request_id);
    Ok(())
}

async fn main() {
    setup_bus(Vec::new()).await;

    // let pool = connect to DataBase 
    let mut txn = pool.begin().await.unwrap();
    let metadata = BusMetadata::default();
    let mut ctx = ExampleBusContext::new(&mut txn, metadata);

    bus::event(&mut ctx, UserRegistered { id: 42 }).await.unwrap();
}

```

### ⚡ **Immutable** vs **Mutable** AccessOne of the most powerful features of **rust_bus** is **Dynamic
** Access Control.

The **bus** understands the difference between a **read-only** context and a **mutable** one.
Access TypeHandler SignatureWhen to useImmutable **&ctxSending** emails, logging, read-only DB queries.
Mutable **&mut ctxDatabase** inserts/updates, modifying metadata.

The Safety Rule: If you dispatch with **&ctx**, the bus will only execute handlers that require **&ctx**. If a handler
requires **&mut ctx**, the bus will return a **BusError**.
If you dispatch with **&mut ctx**, the bus will execute all handlers (both **&** and **&mut**).

### 📊 Distributed Tracing with MetadataThe BusMetadata inside the context is designed for observability.

It supports standard tracing fields and **custom values**.

```rust
let mut meta = BusMetadata::default ();
meta.add("request_id", uuid::Uuid::now_v7()) ?;
meta.add("user_role", "admin") ?; // Custom dynamic metadata

let ctx = ExampleBusContext::new( & txn, meta);
```

In **any handler**, you can retrieve these values:

```rust
let role: String = ctx.metadata().try_get("user_role") ?;
```

### 🗄 Database & Transaction Propagation

The **context** system ensures that your **database transactions** are **shared** across **all handlers** in the chain.

Using Sea-ORM

```rust
#[BusEventHandler]
async fn handle_db(ctx: &ExampleBusContext<'_>, event: &SaveEvent) -> BusResult {
    // Access the shared transaction
    let txn = ctx.txn();
    // Use txn with Sea-ORM models...
    Ok(())
}

```

Using SQLx

```rust
#[BusEventHandler]
async fn handle_db(ctx: &mut ExampleBusContext<'_, '_>, event: &SaveEvent) -> BusResult {
    sqlx::query("INSERT INTO logs (msg) VALUES ($1)")
        .bind(&event.msg)
        .execute(&mut **ctx.txn()) // Access raw SQLx transaction
        .await?;
    Ok(())
}

```

### 🔄 Nested Events (The Power of Chains)

**Contexts** are designed to be passed down. When one handler triggers another event,
the **context** (and its **transaction/metadata**) follows.

```rust
#[BusEventHandler]
async fn handle_order_placed(ctx: &mut MyContext, event: &OrderPlaced) -> BusResult {
    // 1. This uses the existing transaction
    db::reduce_inventory(ctx.txn(), event.product_id).await?;

    // 2. Trigger next event using the SAME context
    // All downstream handlers will share the same transaction and request_id
    bus::event(ctx, NotifyWarehouse { id: event.id }).await?;

    Ok(())
}

```

## 🏗 Creating Your Own ContextWhile

**ExampleBusContext** is great for getting **started**, you will likely want to create a **custom context** for your
production app.

1. Define the struct

```rust
pub struct AppContext<'a> {
    txn: &'a DatabaseTransaction,
    metadata: BusMetadata,
    pub tenant_id: String, // Your custom application data
}

```

2. Implement

```rust
IBusContextRustimpl<'a > IBusContext<'a > for AppContext<'a > {
    fn metadata( & self ) -> & BusMetadata {
        & self.metadata
    }

    // Required if you use DB features
    fn txn( & self ) -> & 'a DatabaseTransaction {
        self.txn
    }
}
```

### 🏆 Best Practices 

#### Context is Request-Scoped:

Create a **new context** at the **start** of an **HTTP request** or a **Background Job** and **drop** it when **finished**.

#### Pass by Reference: 

Always pass the **context** to **bus::event** by reference (**&ctx** or **&mut ctx**).
The library is optimized to handle this without cloning.
Use **&ctx** by **Default**: Only request **&mut ctx** in handlers that
actually perform database writes. This makes your handlers easier to test and compose.

#### Leverage Metadata for Logs:

Always include **ctx.metadata().request_id** in your handler logs to make debugging easier. 
