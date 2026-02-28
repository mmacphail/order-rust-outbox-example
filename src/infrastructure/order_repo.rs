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
