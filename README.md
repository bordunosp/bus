[![Crates.io](https://img.shields.io/crates/v/rust_bus.svg)](https://crates.io/crates/rust_bus)
[![Docs.rs](https://docs.rs/rust_bus/badge.svg)](https://docs.rs/rust_bus)
[![License](https://img.shields.io/crates/l/rust_bus)](https://crates.io/crates/rust_bus)
![Feature Flags](https://img.shields.io/badge/features-sea--orm%20%7C%20json--payload-blue)
[![Downloads](https://img.shields.io/crates/d/rust_bus.svg?style=flat-square)](https://crates.io/crates/rust_bus)


# 🚌 rust_bus — Lightweight, flexible, and production-ready **Event Bus** for Rust

---

## 🧐 What is `rust_bus`?

In modern software architecture, you often need to decouple your business logic from side effects (like sending emails,
processing images, or updating external APIs).

`rust_bus` a guarantee that a task will be completed even if the server restarts or a network error occurs.

Instead of executing logic immediately, you **enqueue** a job (Event) into a database-backed queue. `rust_bus` workers then pick up these jobs asynchronously, handling retries, timeouts, and concurrency limits automatically.

## 🚀 How it Works

The Bus operates on three main pillars:

1. **Events:** Simple Rust structs that implement `IBusEvent` (carrying your data).
2. **Handlers:** Logic units that implement `IEventHandler` or `IEventHandlerDatabase`.
3. **The Dispatcher:** The engine that matches events to their corresponding handlers.

## 🛠 Why use `rust_bus`?

* **Decoupling:** Your main business flow doesn't need to know who is listening to its events.
* **Reliability:** With the database-backed worker system, if a handler fails or the server crashes, the job is not
  lost. It remains in the DB for a retry.
* **Scalability:** You can run multiple instances of your application (Nodes), and they will coordinate to process jobs
  from the shared database.
* **Flexibility:** Use it purely in-memory for speed, or plug in a database for persistence.

## 📦 Out-Of-The-Box Features

Unlike simple event emitters, `rust_bus` provides a full-featured background job system:

* **Retries with Backoff:** Automatically retry failed jobs with configurable delays ($2^{attempt}$ minutes).
* **Timeouts:** Ensure no job hangs forever and blocks your workers.
* **Priority Queues:** Execute critical tasks (like "Process Payment") before lower priority ones (like "Send
  Newsletter").
* **Graceful Shutdown:** Integrated with Tokio, allowing workers to finish current tasks before stopping.
* **Event Uniqueness:** Prevent duplicate jobs using sophisticated `Unique` constraints. Define uniqueness based on
  specific fields (Event payload, Queue, Meta, etc.) and time periods (Infinite or Duration-based).
* **Smart Replacement:** Define `Replace` strategies to update existing jobs instead of creating new ones. For example,
  if a job is in `Available` or `Retryable` state, you can automatically update its `Payload`, `Priority`, or `Metadata`
  with the latest data.

## 💾 Storage Backends

`rust_bus` is highly modular. You can choose your level of persistence via Cargo features:

### 1. In-Memory (Default)

Fastest performance. Best for UI updates or transient state changes where losing data on crash is acceptable.

### 2. Database-Backed (Persistent)

If you need 100% delivery guarantees, choose one of the supported backends:

| Feature            | Description                                  |
|:-------------------|:---------------------------------------------|
| `sea-orm-postgres` | Advanced ORM support for PostgreSQL          |
| `sea-orm-mysql`    | Advanced ORM support for MySQL               |
| `sqlx-postgres`    | Lightweight async SQL support for PostgreSQL |
| `sqlx-mysql`       | Lightweight async SQL support for MySQL      |

## 🛡 Fault Tolerance

**rust_bus** is designed for production. It handles:

* **Panics**: If a handler panics, the worker catches it, logs it, and marks the job for retry.
* **Timeouts**: If a job exceeds its execution_timeout_sec, the worker kills the task and recovers.
* **Recovery Supervisor**: A dedicated background process that finds "zombie" jobs (e.g., from a node that crashed) and
  puts them back into the queue.

---

## 📦 Migrations for Database Events

If you're using database-backed events with SeaORM or SQLX, you must apply the required database migrations before
running
workers.

All required migrations are located in the root of the repository under the migration/ directory:

```toml
/migration
├── mysql.sql
├── postgres.sql
└── ...
```

[migration](https://github.com/bordunosp/bus/blob/master/migration)

You can apply them using your preferred migration tool (e.g. SeaORM CLI, refinery, sqlx-cli, or manually via psql/MySQL
client).

---

## 🧠 Design Philosophy

* ✅ Explicit over implicit
* ✅ Async-first
* ✅ Minimal boilerplate
* ✅ Composable middleware
* ✅ Type-safe boundaries

---

# 📚 Documentation Index

| Topic                      | File                                                                                                    |
|:---------------------------|:--------------------------------------------------------------------------------------------------------|
| 📣 Events (in-memory)      | [event.md](https://github.com/bordunosp/bus/blob/master/doc/event.md)                                   |
| 🐘️ Events SeaOrm Postgres | [event_sea_orm_postgres.md](https://github.com/bordunosp/bus/blob/master/doc/event_sea_orm_postgres.md) |
| 🐬️ Events SeaOrm Mysql    | [event_sea_orm_mysql.md](https://github.com/bordunosp/bus/blob/master/doc/event_sea_orm_mysql.md)       |
| 🐘️ Events sqlx Postgres   | [event_sqlx_postgres.md](https://github.com/bordunosp/bus/blob/master/doc/event_sqlx_postgres.md)       |
| 🐬️ Events sqlx Mysql      | [event_sqlx_mysql.md](https://github.com/bordunosp/bus/blob/master/doc/event_sqlx_mysql.md)             |
| 🧬️ Context                | [context.md](https://github.com/bordunosp/bus/blob/master/doc/context.md)                               |
| 🏗️ Migrations             | bus app folder [migration](https://github.com/bordunosp/bus/blob/master/migration)                      |

---

# #StandForUkraine 🇺🇦

This project aims to show support for Ukraine and its people amidst a war that has been ongoing since 2014. This war has
a genocidal nature and has led to the deaths of thousands, injuries to millions, and significant property damage. We
believe that the international community should focus on supporting Ukraine and ensuring security and freedom for its
people.

Join us and show your support using the hashtag #StandForUkraine. Together, we can help bring attention to the issues
faced by Ukraine and provide aid.