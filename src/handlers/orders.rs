use actix_web::{web, HttpResponse};
use bigdecimal::BigDecimal;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::str::FromStr;
use uuid::Uuid;

use crate::db::DbPool;
use crate::errors::AppError;
use crate::models::order::NewOrder;
use crate::models::order_line::NewOrderLine;
use crate::models::outbox::NewOutboxEvent;
use crate::schema::{order_lines, orders, outbox};

// ── Request / response DTOs ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateOrderLineRequest {
    pub product_id: Uuid,
    pub quantity: i32,
    pub unit_price: String, // decimal as string to avoid floating-point issues
}

#[derive(Debug, Deserialize)]
pub struct CreateOrderRequest {
    pub customer_id: Uuid,
    pub lines: Vec<CreateOrderLineRequest>,
}

#[derive(Debug, Serialize)]
pub struct OrderLineResponse {
    pub id: Uuid,
    pub product_id: Uuid,
    pub quantity: i32,
    pub unit_price: String,
}

#[derive(Debug, Serialize)]
pub struct OrderResponse {
    pub id: Uuid,
    pub customer_id: Uuid,
    pub status: String,
    pub created_at: String,
    pub lines: Vec<OrderLineResponse>,
}

// ── Handlers ─────────────────────────────────────────────────────────────────

/// POST /orders
///
/// Creates a new order together with its order lines. All inserts (order,
/// order_lines, and an outbox event) are performed inside a single database
/// transaction so that the outbox entry is guaranteed to be written if and
/// only if the order is committed.
pub async fn create_order(
    pool: web::Data<DbPool>,
    body: web::Json<CreateOrderRequest>,
) -> Result<HttpResponse, AppError> {
    let body = body.into_inner();

    let result = web::block(move || {
        let mut conn = pool.get()?;

        conn.transaction::<_, AppError, _>(|conn| {
            // 1. Insert the order
            let order_id = Uuid::new_v4();
            let new_order = NewOrder {
                id: order_id,
                customer_id: body.customer_id,
                status: "PENDING".to_string(),
            };
            diesel::insert_into(orders::table)
                .values(&new_order)
                .execute(conn)?;

            // 2. Insert order lines
            let new_lines: Result<Vec<NewOrderLine>, AppError> = body
                .lines
                .iter()
                .map(|l| {
                    let price = BigDecimal::from_str(&l.unit_price).map_err(|e| {
                        AppError::Internal(format!("Invalid unit_price '{}': {}", l.unit_price, e))
                    })?;
                    Ok(NewOrderLine {
                        id: Uuid::new_v4(),
                        order_id,
                        product_id: l.product_id,
                        quantity: l.quantity,
                        unit_price: price,
                    })
                })
                .collect();
            let new_lines = new_lines?;
            diesel::insert_into(order_lines::table)
                .values(&new_lines)
                .execute(conn)?;

            // 3. Build the outbox payload and insert the event.
            //    Debezium's outbox event router will read this row via CDC and
            //    publish the payload to the Kafka topic derived from
            //    `aggregate_type` (e.g. "Order" → topic "Order").
            let line_payloads: Vec<serde_json::Value> = body
                .lines
                .iter()
                .map(|l| {
                    json!({
                        "product_id": l.product_id,
                        "quantity": l.quantity,
                        "unit_price": l.unit_price
                    })
                })
                .collect();

            let event_payload = json!({
                "order_id": order_id,
                "customer_id": body.customer_id,
                "status": "PENDING",
                "lines": line_payloads
            });

            let new_event = NewOutboxEvent {
                id: Uuid::new_v4(),
                aggregate_type: "Order".to_string(),
                aggregate_id: order_id.to_string(),
                event_type: "OrderCreated".to_string(),
                payload: event_payload,
            };
            diesel::insert_into(outbox::table)
                .values(&new_event)
                .execute(conn)?;

            Ok(order_id)
        })
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(HttpResponse::Created().json(json!({ "id": result })))
}

/// GET /orders/{id}
///
/// Returns the order together with its order lines.
pub async fn get_order(
    pool: web::Data<DbPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let order_id = path.into_inner();

    let result = web::block(move || {
        let mut conn = pool.get()?;

        let order = orders::table
            .filter(orders::id.eq(order_id))
            .select(crate::models::order::Order::as_select())
            .first(&mut conn)
            .optional()?;

        let Some(order) = order else {
            return Ok::<_, AppError>(None);
        };

        let lines = order_lines::table
            .filter(order_lines::order_id.eq(order.id))
            .select(crate::models::order_line::OrderLine::as_select())
            .load(&mut conn)?;

        let line_responses: Vec<OrderLineResponse> = lines
            .into_iter()
            .map(|l| OrderLineResponse {
                id: l.id,
                product_id: l.product_id,
                quantity: l.quantity,
                unit_price: l.unit_price.to_string(),
            })
            .collect();

        Ok(Some(OrderResponse {
            id: order.id,
            customer_id: order.customer_id,
            status: order.status,
            created_at: order.created_at.to_rfc3339(),
            lines: line_responses,
        }))
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    match result {
        Some(order) => Ok(HttpResponse::Ok().json(order)),
        None => Err(AppError::NotFound),
    }
}
