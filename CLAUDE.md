# CLAUDE.md – Project Instructions for Claude Agents

## Project Overview

This is a Rust backend service demonstrating the **Transactional Outbox Pattern**. It exposes a REST API for managing orders, writing domain events to an outbox table within the same database transaction. Debezium (CDC) reads the outbox table and publishes Avro-encoded messages to Kafka.

**Key technologies:**
- **Rust** (edition 2021) – application language
- **actix-web 4** – HTTP framework
- **Diesel 2** – PostgreSQL ORM with compile-time query safety
- **PostgreSQL** – primary datastore with logical replication enabled
- **Debezium** – CDC connector that streams outbox rows to Kafka
- **Apache Kafka** + **Confluent Schema Registry** – event streaming with Avro encoding
- **utoipa** / **utoipa-swagger-ui** – OpenAPI 3 documentation served at `/swagger-ui/`

## Repository Layout

```
src/
  main.rs          – entry point; reads env vars, creates DB pool, starts server
  lib.rs           – registers routes, runs migrations, builds actix-web Server
  db.rs            – r2d2 connection pool helpers
  errors.rs        – thiserror-based AppError enum
  schema.rs        – Diesel-generated table! macros
  handlers/
    mod.rs
    orders.rs      – POST /orders and GET /orders/{id} handlers
  models/
    mod.rs
    order.rs       – Order struct (Queryable / Insertable)
    order_line.rs  – OrderLine struct
    outbox.rs      – NewOutboxEvent for writing to the outbox table
migrations/        – SQL migration files (applied automatically on startup)
tests/
  e2e_test.rs      – end-to-end integration tests (require running infrastructure)
debezium/          – Debezium connector configuration JSON
scripts/           – helper shell scripts (e.g. run_e2e_tests.sh)
justfile           – task runner recipes (build, test, lint, infra, etc.)
docker-compose.yml – full local stack definition
```

## Development Workflow

Use `just` (https://github.com/casey/just) as the task runner:

| Command | Description |
|---|---|
| `just build` | `cargo build` |
| `just check` | `cargo check` |
| `just lint` | `cargo clippy -- -D warnings` |
| `just fmt` | `cargo fmt` |
| `just fmt-check` | `cargo fmt -- --check` |
| `just test` | `cargo test` (unit tests; no infrastructure needed) |
| `just test-e2e` | end-to-end tests (starts/stops Docker Compose) |
| `just infra-up` | start Postgres + Kafka + Debezium in Docker |
| `just up` | full stack including the order service |
| `just down` | tear down all containers and volumes |

## Pre-Commit Checklist

**Before committing or submitting changes always run (in this order):**
```bash
just fmt-check
just lint
just test
just test-e2e
```

> **Note:** `just test-e2e` requires Docker. It starts the full infrastructure stack, runs the integration tests, and tears everything down. Never skip it.

To automatically enforce the fast checks (format + lint + unit tests) on every commit, install the provided git hook once:
```bash
just install-hooks
```

## Coding Conventions

- Follow standard Rust idioms and the existing code style.
- All `clippy` warnings are treated as errors (`-D warnings`); fix them before opening a PR.
- Use `thiserror` for error types; add variants to `AppError` in `errors.rs` rather than using `anyhow` or `.unwrap()` in handler code.
- Database queries use Diesel's type-safe DSL. Add new columns/tables via a new migration file; never modify existing migration files.
- New API endpoints must be registered in `lib.rs` (`build_server`) and annotated with `utoipa` macros so they appear in the OpenAPI spec.
- Outbox events are written to the `outbox` table inside the same transaction as the domain write. Do not write events outside of a transaction.
- Keep `schema.rs` in sync with migrations by running `diesel print-schema` after adding migrations.

## Environment Variables

| Variable | Default | Description |
|---|---|---|
| `DATABASE_URL` | `postgres://order_user:order_pass@localhost/order_db` | PostgreSQL connection string |
| `HOST` | `0.0.0.0` | Bind address |
| `PORT` | `8080` | HTTP port |
| `RUST_LOG` | `info` | Log level |

Copy `.env.example` to `.env` for local development (`just env`).

## Testing

- **Unit tests** live inside `src/` modules (standard `#[cfg(test)]` blocks) and require only `cargo test`.
- **End-to-end tests** live in `tests/e2e_test.rs`, start the full Docker Compose stack, and exercise the HTTP API + Kafka consumer.
- Do not rely on `unwrap()` in test assertions; use `expect("…")` with a descriptive message.

## Outbox Pattern – Key Invariants

1. Every write to `orders` / `order_lines` **must** include a corresponding insert into `outbox` in the **same database transaction**.
2. The `aggregate_type` column in `outbox` determines the Kafka topic name.
3. The `payload` column stores a JSON value; Debezium serializes it as an Avro `string` using the Confluent wire format.
