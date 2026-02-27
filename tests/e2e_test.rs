//! End-to-end test: POST /orders → Debezium CDC → Avro → Kafka topic "Order".
//!
//! Requires the full infrastructure stack to be running before executing:
//!
//!   docker compose up -d postgres kafka schema-registry debezium
//!
//! The easiest way to run this test is via the helper script:
//!
//!   ./scripts/run_e2e_tests.sh
//!
//! Or start infrastructure manually and run with:
//!
//!   DATABASE_URL=postgres://order_user:order_pass@localhost:5432/order_db \
//!     cargo test --test e2e_test -- --include-ignored

use futures::StreamExt;
use order_service::{build_server, create_pool, run_migrations};
use rdkafka::consumer::{Consumer, StreamConsumer};
use rdkafka::message::Message;
use rdkafka::ClientConfig;
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;
use uuid::Uuid;

const DEBEZIUM_URL: &str = "http://localhost:8083";
const SCHEMA_REGISTRY_URL: &str = "http://localhost:8081";
const KAFKA_BROKERS: &str = "localhost:9092";
const KAFKA_TOPIC: &str = "Order";
const APP_PORT: u16 = 18080;
const KAFKA_WAIT_SECS: u64 = 60;

/// Wait until `url` returns an HTTP 2xx, retrying every `interval` for up to
/// `timeout` total. Panics if the service never becomes healthy.
async fn wait_for_http(label: &str, url: &str, timeout: Duration, interval: Duration) {
    let client = Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if tokio::time::Instant::now() > deadline {
            panic!("{} did not become ready within {:?}", label, timeout);
        }
        // Any HTTP response (even 4xx) means the server is up.
        if client.get(url).send().await.is_ok() {
            return;
        }
        tokio::time::sleep(interval).await;
    }
}

/// Register (or replace) the Debezium outbox connector.
///
/// The connector is configured to read `public.outbox` from the Postgres
/// container (reachable inside Docker as "postgres") and publish events to
/// a Kafka topic named after `aggregate_type` (e.g. "Order").
async fn register_debezium_connector(http: &Client) {
    // Remove any stale connector so registration is idempotent.
    let _ = http
        .delete(format!("{}/connectors/order-outbox-connector", DEBEZIUM_URL))
        .send()
        .await;
    tokio::time::sleep(Duration::from_secs(2)).await;

    let connector_config = json!({
        "name": "order-outbox-connector",
        "config": {
            "connector.class": "io.debezium.connector.postgresql.PostgresConnector",
            // "postgres" is the service name inside the Docker Compose network.
            "database.hostname": "postgres",
            "database.port": "5432",
            "database.user": "order_user",
            "database.password": "order_pass",
            "database.dbname": "order_db",
            "topic.prefix": "order_db_e2e",
            "plugin.name": "pgoutput",
            "slot.name": "e2e_slot",
            "publication.name": "e2e_pub",
            "table.include.list": "public.outbox",
            "tombstones.on.delete": "false",
            "transforms": "outbox",
            "transforms.outbox.type": "io.debezium.transforms.outbox.EventRouter",
            "transforms.outbox.table.field.event.id": "id",
            "transforms.outbox.table.field.event.key": "aggregate_id",
            "transforms.outbox.table.field.event.type": "event_type",
            "transforms.outbox.table.field.event.payload": "payload",
            "transforms.outbox.route.by.field": "aggregate_type",
            "transforms.outbox.route.topic.replacement": "${routedByValue}",
            "key.converter": "org.apache.kafka.connect.storage.StringConverter",
            "value.converter": "io.apicurio.registry.utils.converter.AvroConverter",
            "value.converter.apicurio.registry.url": "http://schema-registry:8080/apis/registry/v2",
            "value.converter.apicurio.registry.auto-register": "true",
            "value.converter.apicurio.registry.find-latest": "true",
            "value.converter.apicurio.registry.id-handler": "io.apicurio.registry.serde.Legacy4ByteIdHandler"
        }
    });

    let resp = http
        .post(format!("{}/connectors", DEBEZIUM_URL))
        .json(&connector_config)
        .send()
        .await
        .expect("Failed to POST connector to Debezium");

    assert!(
        resp.status().is_success(),
        "Debezium connector registration failed ({}): {}",
        resp.status(),
        resp.text().await.unwrap_or_default()
    );
}

/// Poll the Debezium connector status until it reports RUNNING.
async fn wait_for_connector_running(http: &Client) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        if tokio::time::Instant::now() > deadline {
            panic!("Debezium connector did not reach RUNNING state within 30 s");
        }
        let resp = http
            .get(format!(
                "{}/connectors/order-outbox-connector/status",
                DEBEZIUM_URL
            ))
            .send()
            .await;

        if let Ok(r) = resp {
            if let Ok(v) = r.json::<Value>().await {
                if v["connector"]["state"].as_str() == Some("RUNNING") {
                    return;
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

// ── Test ──────────────────────────────────────────────────────────────────────

/// Full end-to-end flow:
///  1. Start the order service (actix-web) in a background task.
///  2. Register the Debezium outbox connector (Avro + Apicurio Schema Registry).
///  3. POST a new order via the REST API.
///  4. Consume the Kafka "Order" topic until the `OrderCreated` event matching
///     the new order's ID is received (up to 60 seconds).
///
/// Messages are serialized as Avro using the Confluent wire format
/// (magic byte 0x00 + 4-byte schema ID + Avro-encoded payload).
#[tokio::test]
#[ignore = "requires docker-compose infrastructure – run via scripts/run_e2e_tests.sh"]
async fn test_create_order_event_reaches_kafka() {
    let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://order_user:order_pass@localhost:5432/order_db".to_string()
    });

    // ── 1. Start the order service ───────────────────────────────────────────
    let pool = create_pool(&database_url);
    run_migrations(&pool);

    let server = build_server(pool, "127.0.0.1", APP_PORT)
        .expect("Failed to bind the order service");
    tokio::spawn(server);

    let app_url = format!("http://127.0.0.1:{}", APP_PORT);

    // Wait for the server to be ready (any non-connect-error response is fine).
    wait_for_http(
        "order service",
        &format!("{}/orders", app_url),
        Duration::from_secs(10),
        Duration::from_millis(300),
    )
    .await;

    let http = Client::new();

    // ── 2. Register the Debezium connector ──────────────────────────────────
    wait_for_http(
        "Schema Registry",
        &format!("{}/health/ready", SCHEMA_REGISTRY_URL),
        Duration::from_secs(60),
        Duration::from_secs(2),
    )
    .await;

    wait_for_http(
        "Debezium Connect",
        &format!("{}/connectors", DEBEZIUM_URL),
        Duration::from_secs(60),
        Duration::from_secs(2),
    )
    .await;

    register_debezium_connector(&http).await;
    wait_for_connector_running(&http).await;

    // ── 3. Create a Kafka consumer before posting the order ──────────────────
    // A unique group-id per test run ensures offset tracking starts fresh and
    // `auto.offset.reset = "earliest"` ensures we read from the beginning of
    // the topic partition even if the message was produced before subscribe().
    let consumer: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", KAFKA_BROKERS)
        .set("group.id", format!("e2e-{}", Uuid::new_v4()))
        .set("auto.offset.reset", "earliest")
        .set("enable.auto.commit", "false")
        .create()
        .expect("Failed to create Kafka consumer");

    consumer
        .subscribe(&[KAFKA_TOPIC])
        .expect("Failed to subscribe to Kafka topic");

    // ── 4. POST /orders ──────────────────────────────────────────────────────
    let customer_id = Uuid::new_v4();
    let product_id = Uuid::new_v4();

    let create_resp = http
        .post(format!("{}/orders", app_url))
        .json(&json!({
            "customer_id": customer_id,
            "lines": [
                {
                    "product_id": product_id,
                    "quantity": 3,
                    "unit_price": "29.99"
                }
            ]
        }))
        .send()
        .await
        .expect("Failed to POST /orders");

    assert_eq!(
        create_resp.status(),
        201,
        "Expected 201 Created from POST /orders"
    );

    let body: Value = create_resp
        .json()
        .await
        .expect("Failed to parse POST /orders response body");
    let order_id = body["id"]
        .as_str()
        .expect("Response body missing 'id' field")
        .to_string();

    println!("Created order id={}", order_id);

    // ── 5. Poll Kafka until the matching OrderCreated event appears ──────────
    let deadline = tokio::time::Instant::now() + Duration::from_secs(KAFKA_WAIT_SECS);
    let mut kafka_stream = consumer.stream();
    let mut found = false;

    loop {
        if tokio::time::Instant::now() > deadline {
            break;
        }

        let msg = match tokio::time::timeout(Duration::from_secs(5), kafka_stream.next()).await {
            Ok(Some(Ok(m))) => m,
            Ok(Some(Err(e))) => {
                eprintln!("Kafka error: {}", e);
                continue;
            }
            _ => continue,
        };

        let raw_bytes = match msg.payload() {
            Some(b) => b,
            None => continue,
        };

        // Messages are Avro-encoded using the Confluent/Apicurio wire format:
        // byte 0    – magic byte (0x00)
        // bytes 1–4 – 4-byte big-endian schema/artifact ID
        // bytes 5+  – Avro binary-encoded string (the JSONB payload)
        let json_str = match decode_avro_string_payload(raw_bytes) {
            Some(s) => s,
            None => {
                eprintln!(
                    "Failed to decode Avro payload ({} bytes)",
                    raw_bytes.len()
                );
                continue;
            }
        };

        let event: Value = match serde_json::from_str(&json_str) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("Failed to parse Kafka payload as JSON: {}", e);
                continue;
            }
        };

        println!("Received Kafka event: {}", event);

        if event["order_id"].as_str() != Some(order_id.as_str()) {
            // Message belongs to a different order (e.g. leftover from a
            // previous run); keep consuming.
            continue;
        }

        // ── Assertions ───────────────────────────────────────────────────────
        assert_eq!(
            event["status"].as_str(),
            Some("PENDING"),
            "OrderCreated event should have status PENDING"
        );
        assert_eq!(
            event["customer_id"].as_str(),
            Some(customer_id.to_string().as_str()),
            "OrderCreated event customer_id mismatch"
        );

        let lines = event["lines"]
            .as_array()
            .expect("OrderCreated event 'lines' should be an array");
        assert_eq!(lines.len(), 1, "Expected exactly 1 order line in event");
        assert_eq!(
            lines[0]["product_id"].as_str(),
            Some(product_id.to_string().as_str()),
            "Order line product_id mismatch"
        );
        assert_eq!(
            lines[0]["quantity"].as_i64(),
            Some(3),
            "Order line quantity mismatch"
        );
        assert_eq!(
            lines[0]["unit_price"].as_str(),
            Some("29.99"),
            "Order line unit_price mismatch"
        );

        found = true;
        break;
    }

    assert!(
        found,
        "OrderCreated event for order '{}' was not received on Kafka topic '{}' within {} seconds",
        order_id, KAFKA_TOPIC, KAFKA_WAIT_SECS
    );
}

// ── Avro wire format helpers ──────────────────────────────────────────────────

/// Decode an Avro-encoded payload from the Confluent/Apicurio wire format.
///
/// Wire format: magic byte (0x00) + 4-byte schema ID + Avro binary string.
/// The Debezium outbox EventRouter publishes the JSONB `payload` column as an
/// Avro string, so decoding yields the raw JSON text of the order event.
fn decode_avro_string_payload(bytes: &[u8]) -> Option<String> {
    // Expect: magic byte 0x00 + 4-byte artifact/schema ID = 5-byte header.
    if bytes.len() < 5 || bytes[0] != 0x00 {
        return None;
    }
    let avro_bytes = &bytes[5..];

    // Avro binary string encoding: zigzag long (byte count) + UTF-8 bytes.
    let (byte_count, header_len) = read_avro_long(avro_bytes)?;
    // A negative byte count means corrupted data (valid Avro strings have non-negative length).
    if byte_count < 0 {
        return None;
    }
    let byte_count = byte_count as usize;
    let end = header_len + byte_count;
    if end > avro_bytes.len() {
        return None;
    }
    String::from_utf8(avro_bytes[header_len..end].to_vec()).ok()
}

/// Read a zigzag-encoded Avro long from the start of `bytes`.
///
/// Returns `(decoded_value, bytes_consumed)`.
fn read_avro_long(bytes: &[u8]) -> Option<(i64, usize)> {
    let mut n: u64 = 0;
    let mut shift = 0u32;
    let mut consumed = 0;
    loop {
        if consumed >= bytes.len() {
            return None;
        }
        let b = bytes[consumed] as u64;
        consumed += 1;
        n |= (b & 0x7F) << shift;
        if b & 0x80 == 0 {
            break;
        }
        shift += 7;
    }
    // Zigzag decode: (n >> 1) XOR -(n & 1)
    let decoded = ((n >> 1) as i64) ^ -((n & 1) as i64);
    Some((decoded, consumed))
}
