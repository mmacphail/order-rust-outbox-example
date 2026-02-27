CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

CREATE TABLE orders (
    id          UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    customer_id UUID NOT NULL,
    status      VARCHAR(50) NOT NULL DEFAULT 'PENDING',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE order_lines (
    id          UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    order_id    UUID NOT NULL REFERENCES orders(id) ON DELETE CASCADE,
    product_id  UUID NOT NULL,
    quantity    INTEGER NOT NULL CHECK (quantity > 0),
    unit_price  NUMERIC(12, 2) NOT NULL CHECK (unit_price >= 0),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX order_lines_order_id_idx ON order_lines(order_id);

-- Outbox table used by Debezium CDC to relay domain events to Kafka.
-- The `aggregate_type` and `aggregate_id` columns allow the Debezium
-- outbox router to extract the Kafka topic and message key automatically.
CREATE TABLE outbox (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    aggregate_type  VARCHAR(255) NOT NULL,
    aggregate_id    VARCHAR(255) NOT NULL,
    event_type      VARCHAR(255) NOT NULL,
    payload         JSONB NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
