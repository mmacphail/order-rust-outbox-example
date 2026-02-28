use actix_web::{web, HttpResponse};
use bigdecimal::BigDecimal;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::str::FromStr;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::db::DbPool;
use crate::errors::AppError;
use crate::models::order::NewOrder;
use crate::models::order_line::NewOrderLine;
use crate::models::outbox::NewOutboxEvent;
use crate::schema::{commerce_order_outbox, order_lines, orders};

// ── Request / response DTOs ──────────────────────────────────────────────────

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateOrderLineRequest {
    pub product_id: Uuid,
    pub quantity: i32,
    /// Decimal price as a string to avoid floating-point issues, e.g. "9.99"
    pub unit_price: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateOrderRequest {
    pub customer_id: Uuid,
    pub lines: Vec<CreateOrderLineRequest>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CreateOrderResponse {
    pub id: Uuid,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct OrderLineResponse {
    pub id: Uuid,
    pub product_id: Uuid,
    pub quantity: i32,
    pub unit_price: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct OrderResponse {
    pub id: Uuid,
    pub customer_id: Uuid,
    pub status: String,
    pub created_at: String,
    pub lines: Vec<OrderLineResponse>,
}

// ── Pagination ───────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, ToSchema)]
pub struct ListOrdersParams {
    /// Page number (1-based). Defaults to 1.
    #[serde(default = "default_page")]
    pub page: i64,
    /// Number of items per page. Defaults to 20, maximum 100.
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_page() -> i64 {
    1
}

fn default_limit() -> i64 {
    20
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ListOrdersResponse {
    pub items: Vec<OrderResponse>,
    pub total: i64,
    pub page: i64,
    pub limit: i64,
}

// ── Handlers ─────────────────────────────────────────────────────────────────

/// POST /orders
///
/// Creates a new order together with its order lines. All inserts (order,
/// order_lines, and an outbox event) are performed inside a single database
/// transaction so that the outbox entry is guaranteed to be written if and
/// only if the order is committed.
#[utoipa::path(
    post,
    path = "/orders",
    request_body = CreateOrderRequest,
    responses(
        (status = 201, description = "Order created successfully", body = CreateOrderResponse),
        (status = 500, description = "Internal server error"),
    ),
    tag = "orders"
)]
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
            //    `aggregate_type` determines the Kafka topic via the EventRouter SMT.
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
            diesel::insert_into(commerce_order_outbox::table)
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
#[utoipa::path(
    get,
    path = "/orders/{id}",
    params(
        ("id" = Uuid, Path, description = "Order UUID"),
    ),
    responses(
        (status = 200, description = "Order found", body = OrderResponse),
        (status = 404, description = "Order not found"),
        (status = 500, description = "Internal server error"),
    ),
    tag = "orders"
)]
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

/// GET /orders
///
/// Returns a paginated list of orders (without their lines).
/// Use `page` (1-based) and `limit` to control pagination.
#[utoipa::path(
    get,
    path = "/orders",
    params(
        ("page" = Option<i64>, Query, description = "Page number (1-based, default 1)"),
        ("limit" = Option<i64>, Query, description = "Items per page (default 20, max 100)"),
    ),
    responses(
        (status = 200, description = "Paginated list of orders", body = ListOrdersResponse),
        (status = 500, description = "Internal server error"),
    ),
    tag = "orders"
)]
pub async fn list_orders(
    pool: web::Data<DbPool>,
    query: web::Query<ListOrdersParams>,
) -> Result<HttpResponse, AppError> {
    let params = query.into_inner();
    let page = params.page.max(1);
    let limit = params.limit.clamp(1, 100);
    let offset = (page - 1) * limit;

    let result = web::block(move || {
        let mut conn = pool.get()?;

        let total: i64 = orders::table.count().get_result(&mut conn)?;

        let rows = orders::table
            .select(crate::models::order::Order::as_select())
            .order(orders::created_at.desc())
            .limit(limit)
            .offset(offset)
            .load(&mut conn)?;

        let items: Vec<OrderResponse> = rows
            .into_iter()
            .map(|o| OrderResponse {
                id: o.id,
                customer_id: o.customer_id,
                status: o.status,
                created_at: o.created_at.to_rfc3339(),
                lines: vec![],
            })
            .collect();

        Ok::<_, AppError>(ListOrdersResponse {
            items,
            total,
            page,
            limit,
        })
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(HttpResponse::Ok().json(result))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── default_page / default_limit ──────────────────────────────────────────

    #[test]
    fn default_page_is_one() {
        assert_eq!(default_page(), 1);
    }

    #[test]
    fn default_limit_is_twenty() {
        assert_eq!(default_limit(), 20);
    }

    // ── ListOrdersParams deserialization ──────────────────────────────────────

    #[test]
    fn list_orders_params_uses_defaults_when_fields_absent() {
        let params: ListOrdersParams =
            serde_json::from_str("{}").expect("deserialize empty object");
        assert_eq!(params.page, 1);
        assert_eq!(params.limit, 20);
    }

    #[test]
    fn list_orders_params_accepts_custom_values() {
        let params: ListOrdersParams =
            serde_json::from_str(r#"{"page":3,"limit":50}"#).expect("deserialize custom values");
        assert_eq!(params.page, 3);
        assert_eq!(params.limit, 50);
    }

    // ── Pagination arithmetic ─────────────────────────────────────────────────

    #[test]
    fn page_clamped_to_minimum_one() {
        assert_eq!(0i64.max(1), 1);
        assert_eq!((-5i64).max(1), 1);
        assert_eq!(1i64.max(1), 1);
        assert_eq!(5i64.max(1), 5);
    }

    #[test]
    fn limit_clamped_to_one_to_one_hundred() {
        assert_eq!(0i64.clamp(1, 100), 1);
        assert_eq!(101i64.clamp(1, 100), 100);
        assert_eq!(1i64.clamp(1, 100), 1);
        assert_eq!(100i64.clamp(1, 100), 100);
        assert_eq!(42i64.clamp(1, 100), 42);
    }

    #[test]
    fn offset_is_zero_on_first_page() {
        let page: i64 = 1;
        let limit: i64 = 20;
        assert_eq!((page - 1) * limit, 0);
    }

    #[test]
    fn offset_advances_by_limit_each_page() {
        let limit: i64 = 20;
        assert_eq!((2i64 - 1) * limit, 20);
        assert_eq!((3i64 - 1) * limit, 40);
        assert_eq!((5i64 - 1) * limit, 80);
    }

    // ── CreateOrderRequest deserialization ────────────────────────────────────

    #[test]
    fn create_order_request_deserializes_with_lines() {
        let customer = Uuid::new_v4();
        let product = Uuid::new_v4();
        let json = serde_json::json!({
            "customer_id": customer,
            "lines": [
                {"product_id": product, "quantity": 2, "unit_price": "9.99"}
            ]
        });
        let req: CreateOrderRequest =
            serde_json::from_value(json).expect("deserialize CreateOrderRequest");
        assert_eq!(req.customer_id, customer);
        assert_eq!(req.lines.len(), 1);
        assert_eq!(req.lines[0].product_id, product);
        assert_eq!(req.lines[0].quantity, 2);
        assert_eq!(req.lines[0].unit_price, "9.99");
    }

    #[test]
    fn create_order_request_deserializes_with_empty_lines() {
        let customer = Uuid::new_v4();
        let json = serde_json::json!({"customer_id": customer, "lines": []});
        let req: CreateOrderRequest =
            serde_json::from_value(json).expect("deserialize CreateOrderRequest with empty lines");
        assert_eq!(req.customer_id, customer);
        assert!(req.lines.is_empty());
    }

    // ── Unit price parsing ────────────────────────────────────────────────────

    #[test]
    fn valid_unit_price_parses_successfully() {
        use std::str::FromStr;
        assert!(bigdecimal::BigDecimal::from_str("9.99").is_ok());
        assert!(bigdecimal::BigDecimal::from_str("0.00").is_ok());
        assert!(bigdecimal::BigDecimal::from_str("1000").is_ok());
    }

    #[test]
    fn invalid_unit_price_fails_to_parse() {
        use std::str::FromStr;
        assert!(bigdecimal::BigDecimal::from_str("not-a-number").is_err());
        assert!(bigdecimal::BigDecimal::from_str("").is_err());
        assert!(bigdecimal::BigDecimal::from_str("9.9.9").is_err());
    }

    // ── Response serialization ────────────────────────────────────────────────

    #[test]
    fn order_response_serializes_to_json() {
        let id = Uuid::new_v4();
        let customer_id = Uuid::new_v4();
        let resp = OrderResponse {
            id,
            customer_id,
            status: "PENDING".to_string(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            lines: vec![],
        };
        let json = serde_json::to_value(&resp).expect("serialize OrderResponse");
        assert_eq!(json["id"].as_str(), Some(id.to_string().as_str()));
        assert_eq!(
            json["customer_id"].as_str(),
            Some(customer_id.to_string().as_str())
        );
        assert_eq!(json["status"].as_str(), Some("PENDING"));
        assert_eq!(json["lines"].as_array().map(|a| a.len()), Some(0));
    }

    #[test]
    fn order_line_response_serializes_to_json() {
        let id = Uuid::new_v4();
        let product_id = Uuid::new_v4();
        let line = OrderLineResponse {
            id,
            product_id,
            quantity: 3,
            unit_price: "19.99".to_string(),
        };
        let json = serde_json::to_value(&line).expect("serialize OrderLineResponse");
        assert_eq!(json["quantity"].as_i64(), Some(3));
        assert_eq!(json["unit_price"].as_str(), Some("19.99"));
    }

    #[test]
    fn create_order_response_serializes_id() {
        let id = Uuid::new_v4();
        let resp = CreateOrderResponse { id };
        let json = serde_json::to_value(&resp).expect("serialize CreateOrderResponse");
        assert_eq!(json["id"].as_str(), Some(id.to_string().as_str()));
    }

    #[test]
    fn list_orders_response_serializes() {
        let resp = ListOrdersResponse {
            items: vec![],
            total: 0,
            page: 1,
            limit: 20,
        };
        let json = serde_json::to_value(&resp).expect("serialize ListOrdersResponse");
        assert_eq!(json["total"].as_i64(), Some(0));
        assert_eq!(json["page"].as_i64(), Some(1));
        assert_eq!(json["limit"].as_i64(), Some(20));
        assert_eq!(json["items"].as_array().map(|a| a.len()), Some(0));
    }
}
