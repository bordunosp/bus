[![Crates.io](https://img.shields.io/crates/v/rust_bus.svg)](https://crates.io/crates/rust_bus)
![Build Status](https://github.com/bordunosp/bus/actions/workflows/rust.yml/badge.svg)
[![Docs.rs](https://docs.rs/rust_bus/badge.svg)](https://docs.rs/rust_bus)
[![License](https://img.shields.io/crates/l/rust_bus)](https://crates.io/crates/rust_bus)
![unsafe-free](https://img.shields.io/badge/unsafe-✗%20no%20unsafe-success)
![Feature Flags](https://img.shields.io/badge/features-sea--orm%20%7C%20json--payload-blue)
[![Downloads](https://img.shields.io/crates/d/rust_bus.svg?style=flat-square)](https://crates.io/crates/rust_bus)


# 🚌 bus — Lightweight CQRS Library for Rust

####
`bus` is a modular, async-first CQRS library for Rust that helps you structure your application around clear boundaries:
`Commands`, `Queries`, and
`Events`. It supports both in-memory and database-backed event processing, middleware pipelines, and graceful shutdown — all with minimal boilerplate.

---

## 🧠 What Problems Does bus Solve?

* ❌ Tightly coupled business logic and infrastructure
* ❌ Scattered side effects and background jobs
* ❌ No clear separation between commands, queries, and events
* ❌ Manual registration of handlers and middleware

## ✅ `bus` introduces a clean, extensible architecture for:

* 🧱 Structuring your domain logic with CQRS
* 🔁 Handling commands and queries with type-safe handlers
* 📬 Publishing events (`in-memory` or `persisted`)
* 🧩 Composing middleware pipelines for logging, validation, metrics, etc.
* 🧵 Running background workers for database-backed events

---

## 🧭 What is CQRS?

**CQRS** stands for **Command Query Responsibility Segregation** — a pattern that separates:

* **Commands** — operations that change state (e.g. CreateUser)
* **Queries** — operations that return data (e.g. GetUserById)
* **Events** — notifications that something happened (e.g. UserCreated)

This separation improves clarity, testability, and scalability.

---

# 🧱 Core Concepts

## 🔨 Command

A command represents an intention to change state. It is handled by a single handler and returns a result.

📖 Read more → [command.md](https://github.com/bordunosp/bus/blob/master/doc/command.md)

---

## 🔍 Query

A query retrieves data without modifying state. It is also handled by a single handler.

📖 Read more → [query.md](https://github.com/bordunosp/bus/blob/master/doc/query.md)

---

## 📣 Event

An event represents something that has already happened. It can be:

In-memory — handled immediately during bus::publish(...)

Database-backed — persisted and processed asynchronously by background workers

* 📖 Read more → [event.md](https://github.com/bordunosp/bus/blob/master/doc/event.md)
* 📖 Database-backed
  events → [event_database_sea_orm.md](https://github.com/bordunosp/bus/blob/master/doc/event_database_sea_orm.md)

---

## 🧩 Middleware Pipelines

You can wrap handlers with composable pipelines for:

* Logging
* Validation
* Metrics
* Tracing
* Retry instrumentation

Supported for:

* In-memory events → [event_pipeline.md](https://github.com/bordunosp/bus/blob/master/doc/event_pipeline.md)
* Database-backed events → [event_database_pipeline.md](https://github.com/bordunosp/bus/blob/master/doc/event_database_pipeline.md)
* Requests (commands/queries) → [request_pipeline.md](https://github.com/bordunosp/bus/blob/master/doc/request_pipeline.md)

---

# ⚙️ Installation

Add `bus` to your `Cargo.toml`:

```toml
[dependencies]
bus = { package = "rust_bus", version = "" }
ctor = "0.4" # Required for automatic handler & pipeline registration
```

**Why `ctor`?**

`bus` uses the `ctor` crate to automatically register handlers and pipelines at startup. Without it, nothing will be
wired up.

---

## 📦 Migrations for Database Events

If you're using database-backed events with SeaORM, you must apply the required database migrations before running
workers.

All required migrations are located in the root of the repository under the migration/ directory:

```toml
/migration
├── 2023_..._create_bus_events_table.sql
├── 2023_..._create_bus_events_archive.sql
└── ...
```

[migration](https://github.com/bordunosp/bus/blob/master/migration)

You can apply them using your preferred migration tool (e.g. SeaORM CLI, refinery, sqlx-cli, or manually via psql/MySQL
client).

---

## 🚀 Getting Started

1. Define your command, query, or event
2. Implement the corresponding handler trait
3. Register it using a macro (e.g. #[bus::registry::BusCommandHandler])
4. Optionally define pipelines for cross-cutting concerns
5. Use bus::send(...) or bus::publish(...) to dispatch

---

## 🧠 Design Philosophy

* ✅ Explicit over implicit
* ✅ Async-first
* ✅ Minimal boilerplate
* ✅ Composable middleware
* ✅ Type-safe boundaries

---

# 📚 Documentation Index

| Topic                          | File                                                                                                      |
|:-------------------------------|:----------------------------------------------------------------------------------------------------------|
| 🧭 CQRS Overview               | this file                                                                                                 |
| 🔨 Commands                    | [command.md](https://github.com/bordunosp/bus/blob/master/doc/command.md)                                 |
| 🔍 Queries                     | [query.md](https://github.com/bordunosp/bus/blob/master/doc/query.md)                                     |
| 📣 Events (in-memory)          | [event.md](https://github.com/bordunosp/bus/blob/master/doc/event.md)                                     |
| 🗃️ Events (database-backed)   | [event_database_sea_orm.md](https://github.com/bordunosp/bus/blob/master/doc/event_database_sea_orm.md)   |
| 🧩 Event Pipelines (in-memory) | [event_pipeline.md](https://github.com/bordunosp/bus/blob/master/doc/event_pipeline.md)                   |
| 🧩 Event Pipelines (database)  | [event_database_pipeline.md](https://github.com/bordunosp/bus/blob/master/doc/event_database_pipeline.md) |
| 🧩 Request Pipelines           | [request_pipeline.md](https://github.com/bordunosp/bus/blob/master/doc/request_pipeline.md)               |
| 🛠 Migrations                  | bus app folder [migration](https://github.com/bordunosp/bus/blob/master/migration)                        |

---

# #StandForUkraine 🇺🇦

This project aims to show support for Ukraine and its people amidst a war that has been ongoing since 2014. This war has
a genocidal nature and has led to the deaths of thousands, injuries to millions, and significant property damage. We
believe that the international community should focus on supporting Ukraine and ensuring security and freedom for its
people.

Join us and show your support using the hashtag #StandForUkraine. Together, we can help bring attention to the issues
faced by Ukraine and provide aid.

