use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use crate::db::DbPool;
use crate::domain::errors::DomainError;
use crate::domain::order::{ListResult, OrderLineInput, OrderLineView, OrderView};
use crate::domain::ports::OrderRepository;
use crate::schema::{commerce_order_outbox, order_lines, orders};

use super::models::{NewOrderLineRow, NewOrderRow, NewOutboxEventRow, OrderLineRow, OrderRow};

// ── Error conversions (infrastructure concern only) ──────────────────────────

impl From<diesel::result::Error> for DomainError {
    fn from(e: diesel::result::Error) -> Self {
        DomainError::Internal(e.to_string())
    }
}

impl From<r2d2::Error> for DomainError {
    fn from(e: r2d2::Error) -> Self {
        DomainError::Internal(e.to_string())
    }
}

// ── Repository ────────────────────────────────────────────────────────────────

pub struct DieselOrderRepository {
    pool: DbPool,
}

impl DieselOrderRepository {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

impl OrderRepository for DieselOrderRepository {
    fn create(&self, customer_id: Uuid, lines: Vec<OrderLineInput>) -> Result<Uuid, DomainError> {
        let mut conn = self.pool.get()?;

        conn.transaction::<_, DomainError, _>(|conn| {
            // 1. Insert the order
            let order_id = Uuid::new_v4();
            diesel::insert_into(orders::table)
                .values(&NewOrderRow {
                    id: order_id,
                    customer_id,
                    status: "PENDING".to_string(),
                })
                .execute(conn)?;

            // 2. Insert order lines
            let new_lines: Vec<NewOrderLineRow> = lines
                .iter()
                .map(|l| NewOrderLineRow {
                    id: Uuid::new_v4(),
                    order_id,
                    product_id: l.product_id,
                    quantity: l.quantity,
                    unit_price: l.unit_price.clone(),
                })
                .collect();
            diesel::insert_into(order_lines::table)
                .values(&new_lines)
                .execute(conn)?;

            // 3. Insert outbox event in the same transaction.
            //    Debezium's EventRouter SMT derives the Kafka topic from `aggregate_type`.
            let line_payloads: Vec<serde_json::Value> = lines
                .iter()
                .map(|l| {
                    json!({
                        "product_id": l.product_id,
                        "quantity": l.quantity,
                        "unit_price": l.unit_price.to_string()
                    })
                })
                .collect();

            let event_payload = json!({
                "order_id": order_id,
                "customer_id": customer_id,
                "status": "PENDING",
                "lines": line_payloads
            });

            diesel::insert_into(commerce_order_outbox::table)
                .values(&NewOutboxEventRow {
                    id: Uuid::new_v4(),
                    aggregate_type: "Order".to_string(),
                    aggregate_id: order_id.to_string(),
                    event_type: "OrderCreated".to_string(),
                    payload: event_payload,
                })
                .execute(conn)?;

            Ok(order_id)
        })
    }

    fn find_by_id(&self, id: Uuid) -> Result<Option<OrderView>, DomainError> {
        let mut conn = self.pool.get()?;

        let order = orders::table
            .filter(orders::id.eq(id))
            .select(OrderRow::as_select())
            .first(&mut conn)
            .optional()?;

        let Some(order) = order else {
            return Ok(None);
        };

        let lines = order_lines::table
            .filter(order_lines::order_id.eq(order.id))
            .select(OrderLineRow::as_select())
            .load(&mut conn)?;

        Ok(Some(OrderView {
            id: order.id,
            customer_id: order.customer_id,
            status: order.status,
            created_at: order.created_at,
            lines: lines
                .into_iter()
                .map(|l| OrderLineView {
                    id: l.id,
                    product_id: l.product_id,
                    quantity: l.quantity,
                    unit_price: l.unit_price,
                })
                .collect(),
        }))
    }

    fn list(&self, page: i64, limit: i64) -> Result<ListResult, DomainError> {
        let mut conn = self.pool.get()?;

        let offset = (page - 1) * limit;
        conn.transaction::<_, DomainError, _>(|conn| {
            let total: i64 = orders::table.count().get_result(conn)?;

            let rows = orders::table
                .select(OrderRow::as_select())
                .order(orders::created_at.desc())
                .limit(limit)
                .offset(offset)
                .load(conn)?;

            Ok(ListResult {
                items: rows
                    .into_iter()
                    .map(|o| OrderView {
                        id: o.id,
                        customer_id: o.customer_id,
                        status: o.status,
                        created_at: o.created_at,
                        lines: vec![],
                    })
                    .collect(),
                total,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use bigdecimal::BigDecimal;
    use diesel::prelude::*;
    use diesel_migrations::MigrationHarness;
    use testcontainers::core::{ContainerPort, WaitFor};
    use testcontainers::runners::AsyncRunner;
    use testcontainers::{ContainerAsync, GenericImage, ImageExt};
    use uuid::Uuid;

    use super::DieselOrderRepository;
    use crate::db::create_pool;
    use crate::domain::order::OrderLineInput;
    use crate::domain::ports::OrderRepository;
    use crate::infrastructure::models::OutboxEventRow;
    use crate::schema::commerce_order_outbox;

    fn free_port() -> u16 {
        // Bind to port 0 to let the OS assign a free port, then release it.
        // There is a small TOCTOU window, but it is acceptable for test usage.
        std::net::TcpListener::bind("127.0.0.1:0")
            .expect("bind failed")
            .local_addr()
            .expect("addr failed")
            .port()
    }

    async fn setup_db() -> (ContainerAsync<GenericImage>, crate::db::DbPool) {
        // Pre-allocate a host port so we never need `get_host_port_ipv4`, which
        // breaks on Podman because it returns `HostIp: ""` instead of `"0.0.0.0"`.
        let port = free_port();
        let container = GenericImage::new("postgres", "16-alpine")
            .with_wait_for(WaitFor::message_on_stderr(
                "database system is ready to accept connections",
            ))
            .with_mapped_port(port, ContainerPort::Tcp(5432))
            .with_env_var("POSTGRES_USER", "postgres")
            .with_env_var("POSTGRES_PASSWORD", "postgres")
            .with_env_var("POSTGRES_DB", "postgres")
            .start()
            .await
            .expect("Failed to start Postgres container");
        let url = format!("postgres://postgres:postgres@127.0.0.1:{}/postgres", port);
        let pool = create_pool(&url);
        {
            let mut conn = pool.get().expect("Failed to get connection");
            conn.run_pending_migrations(crate::MIGRATIONS)
                .expect("Failed to run migrations");
        }
        (container, pool)
    }

    fn make_line(price: &str) -> OrderLineInput {
        OrderLineInput {
            product_id: Uuid::new_v4(),
            quantity: 2,
            unit_price: BigDecimal::from_str(price).expect("valid decimal"),
        }
    }

    #[tokio::test]
    async fn create_and_find_by_id_roundtrip() {
        let (_container, pool) = setup_db().await;
        let repo = DieselOrderRepository::new(pool);
        let customer_id = Uuid::new_v4();

        let order_id = repo
            .create(customer_id, vec![make_line("9.99")])
            .expect("create failed");

        let order = repo
            .find_by_id(order_id)
            .expect("find failed")
            .expect("order should exist");

        assert_eq!(order.id, order_id);
        assert_eq!(order.customer_id, customer_id);
        assert_eq!(order.status, "PENDING");
        assert_eq!(order.lines.len(), 1);
        assert_eq!(order.lines[0].quantity, 2);
    }

    #[tokio::test]
    async fn create_writes_outbox_event_in_same_transaction() {
        let (_container, pool) = setup_db().await;
        let repo = DieselOrderRepository::new(pool.clone());
        let customer_id = Uuid::new_v4();

        let order_id = repo
            .create(customer_id, vec![make_line("4.50")])
            .expect("create failed");

        let mut conn = pool.get().expect("Failed to get connection");
        let events: Vec<OutboxEventRow> = commerce_order_outbox::table
            .filter(commerce_order_outbox::aggregate_id.eq(order_id.to_string()))
            .select(OutboxEventRow::as_select())
            .load(&mut conn)
            .expect("query failed");

        assert_eq!(events.len(), 1, "exactly one outbox event per order");
        assert_eq!(events[0].aggregate_type, "Order");
        assert_eq!(events[0].event_type, "OrderCreated");
        assert_eq!(events[0].aggregate_id, order_id.to_string());
    }

    #[tokio::test]
    async fn find_by_id_returns_none_for_unknown_id() {
        let (_container, pool) = setup_db().await;
        let repo = DieselOrderRepository::new(pool);

        let result = repo
            .find_by_id(Uuid::new_v4())
            .expect("find should not error");

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn list_returns_empty_when_no_orders() {
        let (_container, pool) = setup_db().await;
        let repo = DieselOrderRepository::new(pool);

        let result = repo.list(1, 20).expect("list failed");

        assert_eq!(result.total, 0);
        assert!(result.items.is_empty());
    }

    #[tokio::test]
    async fn list_paginates_correctly() {
        let (_container, pool) = setup_db().await;
        let repo = DieselOrderRepository::new(pool);
        let customer_id = Uuid::new_v4();

        for _ in 0..5 {
            repo.create(customer_id, vec![make_line("1.00")])
                .expect("create failed");
        }

        let page1 = repo.list(1, 3).expect("list page 1 failed");
        assert_eq!(page1.total, 5);
        assert_eq!(page1.items.len(), 3);

        let page2 = repo.list(2, 3).expect("list page 2 failed");
        assert_eq!(page2.total, 5);
        assert_eq!(page2.items.len(), 2);
    }
}
