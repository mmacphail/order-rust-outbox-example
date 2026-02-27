# order-rust-outbox-example

A Rust backend API demonstrating the **Transactional Outbox Pattern** with:

- **[actix-web](https://actix.rs/)** – HTTP API framework
- **[Diesel](https://diesel.rs/)** – PostgreSQL ORM with compile-time query safety
- **[Debezium](https://debezium.io/)** – Change Data Capture (CDC) connector
- **[Apache Kafka](https://kafka.apache.org/)** – Event streaming platform
- **[Confluent Schema Registry](https://docs.confluent.io/platform/current/schema-registry/index.html)** – Avro schema registry
- **PostgreSQL** – Primary datastore with logical replication enabled

## Domain Model

The service manages an **Order** aggregate consisting of:

- `Order` – top-level aggregate with `id`, `customer_id`, `status`, and timestamps
- `OrderLine` – child entity with `product_id`, `quantity`, and `unit_price`

## Architecture – Transactional Outbox Pattern

```
┌─────────────────────────────────────┐
│            Order Service            │
│  POST /orders                       │
│  ┌───────────────────────────────┐  │
│  │  Database Transaction         │  │
│  │  1. INSERT INTO orders        │  │
│  │  2. INSERT INTO order_lines   │  │
│  │  3. INSERT INTO outbox ───────┼──┼──► Debezium CDC
│  └───────────────────────────────┘  │              │
└─────────────────────────────────────┘              ▼
                                              Kafka Topic
                                              "Order"
                                         (Avro-encoded)
```

When an order is created the API writes the order, its lines, and an
`OrderCreated` outbox event **in a single database transaction**.
Debezium reads new rows from the `outbox` table via PostgreSQL logical
replication (CDC) and publishes the `payload` as an **Avro-encoded** message
to the Kafka topic named after the `aggregate_type` column (e.g. `"Order"`).
Schemas are registered and versioned in the **Confluent Schema Registry**.

## Serialization Format

Kafka messages use the **Confluent wire format**:

| Bytes | Content |
|-------|---------|
| 0 | Magic byte (`0x00`) |
| 1–4 | 4-byte schema ID (big-endian int) |
| 5+ | Avro binary-encoded payload |

The `payload` field from the outbox table (PostgreSQL `JSONB`) is serialized as
an Avro `string`. Schemas are auto-registered in Confluent Schema Registry under
the subject `<topic>-value` (e.g. `Order-value`).

## Prerequisites

- [Docker](https://www.docker.com/) & Docker Compose
- [Rust](https://rustup.rs/) ≥ 1.76 (for local development)

## Quick Start

### 1. Start the infrastructure

```bash
docker-compose up -d postgres kafka schema-registry debezium
```

### 2. Run the service locally

```bash
cp .env.example .env
cargo run
```

Or run everything with Docker:

```bash
docker-compose up --build
```

### 3. Register the Debezium connector

Once Debezium Connect is healthy (usually ~30 s after start):

```bash
curl -X POST http://localhost:8083/connectors \
  -H "Content-Type: application/json" \
  -d @debezium/register-connector.json
```

The connector uses `io.confluent.connect.avro.AvroConverter` to
serialize messages and automatically registers schemas in Confluent Schema Registry
at `http://localhost:8081`.

## API Endpoints

### Create an order

```http
POST /orders
Content-Type: application/json

{
  "customer_id": "550e8400-e29b-41d4-a716-446655440000",
  "lines": [
    {
      "product_id": "6ba7b810-9dad-11d1-80b4-00c04fd430c8",
      "quantity": 2,
      "unit_price": "19.99"
    }
  ]
}
```

Response `201 Created`:

```json
{ "id": "a7b9c3d1-0000-0000-0000-000000000001" }
```

### Get an order

```http
GET /orders/{id}
```

Response `200 OK`:

```json
{
  "id": "a7b9c3d1-0000-0000-0000-000000000001",
  "customer_id": "550e8400-e29b-41d4-a716-446655440000",
  "status": "PENDING",
  "created_at": "2024-01-01T00:00:00+00:00",
  "lines": [
    {
      "id": "b1c2d3e4-0000-0000-0000-000000000002",
      "product_id": "6ba7b810-9dad-11d1-80b4-00c04fd430c8",
      "quantity": 2,
      "unit_price": "19.99"
    }
  ]
}
```

## Database Schema

```sql
orders       – Order aggregate root
order_lines  – Order line items (FK → orders.id)
outbox       – Transactional outbox (read by Debezium)
```

Migrations are applied automatically on startup via `diesel_migrations`.

## Environment Variables

| Variable       | Default                                             | Description            |
|----------------|-----------------------------------------------------|------------------------|
| `DATABASE_URL` | `postgres://order_user:order_pass@localhost/order_db` | PostgreSQL connection  |
| `HOST`         | `0.0.0.0`                                           | Bind address           |
| `PORT`         | `8080`                                              | HTTP port              |
| `RUST_LOG`     | `info`                                              | Log level              |
