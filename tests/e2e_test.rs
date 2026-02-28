//! End-to-end test: POST /orders → Debezium CDC → Avro → Kafka topic "public.commerce.order.c2.v1".
//!
//! Requires the full infrastructure stack to be running before executing:
//!
//!   docker-compose up -d postgres kafka schema-registry debezium
//!
//! The easiest way to run this test is via the helper script:
//!
//!   ./scripts/run_e2e_tests.sh
//!
//! Or start infrastructure manually and run with:
//!
//!   DATABASE_URL=postgres://order_user:order_pass@localhost:5432/order_db \
//!     cargo test --test e2e_test -- --include-ignored

use apache_avro::types::Value as AvroValue;
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
const KAFKA_TOPIC: &str = "public.commerce.order.c2.v1";
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
/// The connector is configured to read `public.commerce_order_outbox` from the
/// Postgres container (reachable inside Docker as "postgres") and publish
/// events to the fixed topic `public.commerce.order.c2.v1`.
async fn register_debezium_connector(http: &Client) {
    // Remove any stale connector so registration is idempotent.
    let _ = http
        .delete(format!(
            "{}/connectors/order-outbox-connector",
            DEBEZIUM_URL
        ))
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
            "table.include.list": "public.commerce_order_outbox",
            "tombstones.on.delete": "false",
            "transforms": "outbox",
            "transforms.outbox.type": "io.debezium.transforms.outbox.EventRouter",
            "transforms.outbox.table.field.event.id": "id",
            "transforms.outbox.table.field.event.key": "aggregate_id",
            "transforms.outbox.table.field.event.type": "event_type",
            "transforms.outbox.table.field.event.payload": "payload",
            "transforms.outbox.route.by.field": "aggregate_type",
            "transforms.outbox.route.topic.replacement": "public.commerce.order.c2.v1",
            "transforms.outbox.table.fields.additional.placement": "id:envelope:event_id,event_type:envelope,created_at:envelope:event_date",
            "transforms.outbox.expand.json.payload": "true",
            "key.converter": "org.apache.kafka.connect.storage.StringConverter",
            "value.converter": "io.confluent.connect.avro.AvroConverter",
            "value.converter.schema.registry.url": "http://schema-registry:8081"
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

/// Poll the Debezium connector status until both the connector and its task report RUNNING.
async fn wait_for_connector_running(http: &Client) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        if tokio::time::Instant::now() > deadline {
            panic!("Debezium connector did not reach RUNNING state within 60 s");
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
                let connector_running = v["connector"]["state"].as_str() == Some("RUNNING");
                let task_running = v["tasks"]
                    .as_array()
                    .and_then(|tasks| tasks.first())
                    .and_then(|t| t["state"].as_str())
                    == Some("RUNNING");

                if connector_running && task_running {
                    return;
                }

                // Fail fast if the task is in a terminal FAILED state.
                let task_failed = v["tasks"]
                    .as_array()
                    .and_then(|tasks| tasks.first())
                    .and_then(|t| t["state"].as_str())
                    == Some("FAILED");
                if task_failed {
                    let trace = v["tasks"][0]["trace"]
                        .as_str()
                        .unwrap_or("<no trace>")
                        .lines()
                        .take(5)
                        .collect::<Vec<_>>()
                        .join("\n");
                    panic!("Debezium connector task entered FAILED state:\n{}", trace);
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

// ── Test ──────────────────────────────────────────────────────────────────────

/// Full end-to-end flow:
///  1. Start the order service (actix-web) in a background task.
///  2. Register the Debezium outbox connector (Avro + Confluent Schema Registry).
///  3. POST a new order via the REST API.
///  4. Consume the Kafka "public.commerce.order.c2.v1" topic until the
///     `OrderCreated` event matching the new order's ID is received (up to 60 s).
///
/// Messages are Avro records with envelope fields (`event_id`, `event_type`,
/// `event_date`) plus a `payload` string containing the order JSON.
#[tokio::test]
#[ignore = "requires docker-compose infrastructure – run via scripts/run_e2e_tests.sh"]
async fn test_create_order_event_reaches_kafka() {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://order_user:order_pass@localhost:5432/order_db".to_string());

    // ── 1. Start the order service ───────────────────────────────────────────
    let pool = create_pool(&database_url);
    run_migrations(&pool);

    let server =
        build_server(pool, "127.0.0.1", APP_PORT).expect("Failed to bind the order service");
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
        &format!("{}/subjects", SCHEMA_REGISTRY_URL),
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

        // Messages are Avro records using the Confluent wire format:
        // byte 0    – magic byte (0x00)
        // bytes 1–4 – 4-byte big-endian schema ID
        // bytes 5+  – Avro binary-encoded record
        let record = match decode_avro_record(raw_bytes, &http).await {
            Some(r) => r,
            None => {
                eprintln!("Failed to decode Avro record ({} bytes)", raw_bytes.len());
                continue;
            }
        };

        // Extract the `payload` field. With `expand.json.payload=true` the
        // EventRouter emits the JSONB payload as a nested Avro record rather
        // than a raw JSON string. `apache_avro::from_value` converts any Avro
        // value (record, map, string, …) to a `serde_json::Value`.
        let payload_avro = match record.get("payload") {
            Some(v) => v,
            None => {
                eprintln!("Avro record missing 'payload' field");
                continue;
            }
        };

        let event: Value = match apache_avro::from_value(payload_avro) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("Failed to convert Avro payload to JSON Value: {}", e);
                continue;
            }
        };

        println!("Received Kafka event: {}", event);

        if event["order_id"].as_str() != Some(order_id.as_str()) {
            // Message belongs to a different order (e.g. leftover from a
            // previous run); keep consuming.
            continue;
        }

        // ── Payload assertions ────────────────────────────────────────────────
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

        // ── Envelope field assertions ─────────────────────────────────────────
        assert_eq!(
            record.get("event_type"),
            Some(&AvroValue::String("OrderCreated".to_string())),
            "Avro envelope event_type mismatch"
        );

        let event_id = match record.get("event_id") {
            Some(AvroValue::String(s)) => s.clone(),
            _ => panic!("Avro envelope missing 'event_id' string field"),
        };
        assert!(
            !event_id.is_empty(),
            "Avro envelope event_id should be non-empty"
        );

        let event_date = match record.get("event_date") {
            Some(AvroValue::String(s)) => s.clone(),
            _ => panic!("Avro envelope missing 'event_date' string field"),
        };
        assert!(
            !event_date.is_empty(),
            "Avro envelope event_date should be non-empty"
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

/// Decode an Avro-encoded record from the Confluent wire format.
///
/// Wire format: magic byte (0x00) + 4-byte big-endian schema ID + Avro binary record.
///
/// The schema is fetched from the Schema Registry at `SCHEMA_REGISTRY_URL` using
/// the ID embedded in the message header, then used to decode the record via
/// `apache_avro`. Returns a field map on success, or `None` on any error.
async fn decode_avro_record(
    bytes: &[u8],
    http: &Client,
) -> Option<std::collections::HashMap<String, AvroValue>> {
    // Validate the 5-byte Confluent wire-format header.
    if bytes.len() < 5 || bytes[0] != 0x00 {
        return None;
    }

    let schema_id = u32::from_be_bytes(bytes[1..5].try_into().ok()?);
    let avro_bytes = &bytes[5..];

    // Fetch the Avro schema JSON from the Schema Registry.
    let schema_url = format!("{}/schemas/ids/{}", SCHEMA_REGISTRY_URL, schema_id);
    let schema_resp: Value = http.get(&schema_url).send().await.ok()?.json().await.ok()?;
    let schema_str = schema_resp["schema"].as_str()?;

    let schema = apache_avro::Schema::parse_str(schema_str).ok()?;

    let value =
        apache_avro::from_avro_datum(&schema, &mut avro_bytes.to_vec().as_slice(), None).ok()?;

    if let AvroValue::Record(fields) = value {
        Some(fields.into_iter().collect())
    } else {
        None
    }
}
