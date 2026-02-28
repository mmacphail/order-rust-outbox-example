use actix_web::{web, HttpResponse};
use bigdecimal::BigDecimal;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::str::FromStr;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::application::order_service::OrderService;
use crate::domain::order::OrderLineInput;
use crate::domain::ports::OrderRepository;
use crate::errors::AppError;

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

impl ListOrdersParams {
    /// Returns `(page, limit)` after clamping inputs to valid ranges.
    pub fn into_query_params(self) -> (i64, i64) {
        let page = self.page.max(1);
        let limit = self.limit.clamp(1, 100);
        (page, limit)
    }
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
pub async fn create_order<R: OrderRepository>(
    service: web::Data<OrderService<R>>,
    body: web::Json<CreateOrderRequest>,
) -> Result<HttpResponse, AppError> {
    let body = body.into_inner();
    let customer_id = body.customer_id;

    // BigDecimal parsing is a presentation-layer concern: validate here before
    // handing off to the domain.
    let lines: Result<Vec<OrderLineInput>, AppError> = body
        .lines
        .iter()
        .map(|l| {
            let unit_price = BigDecimal::from_str(&l.unit_price).map_err(|e| {
                AppError::Internal(format!("Invalid unit_price '{}': {}", l.unit_price, e))
            })?;
            Ok(OrderLineInput {
                product_id: l.product_id,
                quantity: l.quantity,
                unit_price,
            })
        })
        .collect();
    let lines = lines?;

    let svc = service.clone();
    let id = web::block(move || svc.create_order(customer_id, lines))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .map_err(AppError::from)?;

    Ok(HttpResponse::Created().json(json!({ "id": id })))
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
pub async fn get_order<R: OrderRepository>(
    service: web::Data<OrderService<R>>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let order_id = path.into_inner();

    let svc = service.clone();
    let result = web::block(move || svc.get_order(order_id))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .map_err(AppError::from)?;

    match result {
        Some(order) => {
            let line_responses: Vec<OrderLineResponse> = order
                .lines
                .into_iter()
                .map(|l| OrderLineResponse {
                    id: l.id,
                    product_id: l.product_id,
                    quantity: l.quantity,
                    unit_price: l.unit_price.to_string(),
                })
                .collect();
            Ok(HttpResponse::Ok().json(OrderResponse {
                id: order.id,
                customer_id: order.customer_id,
                status: order.status,
                created_at: order.created_at.to_rfc3339(),
                lines: line_responses,
            }))
        }
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
pub async fn list_orders<R: OrderRepository>(
    service: web::Data<OrderService<R>>,
    query: web::Query<ListOrdersParams>,
) -> Result<HttpResponse, AppError> {
    let (page, limit) = query.into_inner().into_query_params();

    let svc = service.clone();
    let result = web::block(move || svc.list_orders(page, limit))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .map_err(AppError::from)?;

    let items: Vec<OrderResponse> = result
        .items
        .into_iter()
        .map(|o| OrderResponse {
            id: o.id,
            customer_id: o.customer_id,
            status: o.status,
            created_at: o.created_at.to_rfc3339(),
            lines: vec![],
        })
        .collect();

    Ok(HttpResponse::Ok().json(ListOrdersResponse {
        items,
        total: result.total,
        page,
        limit,
    }))
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

    // ── ListOrdersParams::into_query_params ───────────────────────────────────

    #[test]
    fn page_below_one_is_clamped_to_one() {
        let (page, _) = ListOrdersParams { page: 0, limit: 20 }.into_query_params();
        assert_eq!(page, 1);
    }

    #[test]
    fn limit_below_one_is_clamped_to_one() {
        let (_, limit) = ListOrdersParams { page: 1, limit: 0 }.into_query_params();
        assert_eq!(limit, 1);
    }

    #[test]
    fn limit_above_one_hundred_is_clamped_to_one_hundred() {
        let (_, limit) = ListOrdersParams {
            page: 1,
            limit: 999,
        }
        .into_query_params();
        assert_eq!(limit, 100);
    }

    #[test]
    fn offset_is_zero_for_first_page() {
        let (page, limit) = ListOrdersParams { page: 1, limit: 20 }.into_query_params();
        let offset = (page - 1) * limit;
        assert_eq!(offset, 0);
    }

    #[test]
    fn offset_advances_by_limit_each_page() {
        let (page, limit) = ListOrdersParams { page: 3, limit: 25 }.into_query_params();
        let offset = (page - 1) * limit;
        assert_eq!(offset, 50);
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

    // ── Handler tests (in-memory stub, no Docker) ─────────────────────────────

    use actix_web::{http::StatusCode, test as actix_test, App};
    use bigdecimal::BigDecimal;
    use chrono::Utc;

    use crate::domain::errors::DomainError;
    use crate::domain::order::{ListResult, OrderLineView, OrderView};

    struct InMemoryOrderRepo {
        find_result: Option<OrderView>,
        create_error: Option<String>,
    }

    impl Default for InMemoryOrderRepo {
        fn default() -> Self {
            Self {
                find_result: None,
                create_error: None,
            }
        }
    }

    impl OrderRepository for InMemoryOrderRepo {
        fn create(
            &self,
            _customer_id: Uuid,
            _lines: Vec<OrderLineInput>,
        ) -> Result<Uuid, DomainError> {
            if let Some(msg) = &self.create_error {
                return Err(DomainError::Internal(msg.clone()));
            }
            Ok(Uuid::new_v4())
        }

        fn find_by_id(&self, _id: Uuid) -> Result<Option<OrderView>, DomainError> {
            Ok(self.find_result.clone())
        }

        fn list(&self, page: i64, limit: i64) -> Result<ListResult, DomainError> {
            Ok(ListResult {
                items: vec![OrderView {
                    id: Uuid::new_v4(),
                    customer_id: Uuid::new_v4(),
                    status: "PENDING".to_string(),
                    created_at: Utc::now(),
                    lines: vec![],
                }],
                total: 1,
            })
            .map(|mut r| {
                // Respect page/limit for the test (trim to empty if out of range)
                if page > 1 {
                    r.items.clear();
                    r.total = 0;
                }
                let _ = limit; // limit is validated by the handler before reaching the repo
                r
            })
        }
    }

    fn make_service<R: OrderRepository>(repo: R) -> web::Data<OrderService<R>> {
        web::Data::new(OrderService::new(repo))
    }

    #[actix_web::test]
    async fn create_order_returns_201_with_id() {
        let svc = make_service(InMemoryOrderRepo::default());
        let app = actix_test::init_service(
            App::new()
                .app_data(svc)
                .route("/orders", web::post().to(create_order::<InMemoryOrderRepo>)),
        )
        .await;

        let req = actix_test::TestRequest::post()
            .uri("/orders")
            .set_json(serde_json::json!({
                "customer_id": Uuid::new_v4(),
                "lines": [{"product_id": Uuid::new_v4(), "quantity": 1, "unit_price": "9.99"}]
            }))
            .to_request();

        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(
            resp.status(),
            StatusCode::CREATED,
            "expected 201 Created for a valid order"
        );
        let body: serde_json::Value = actix_test::read_body_json(resp).await;
        assert!(
            body["id"].is_string(),
            "response body must contain an 'id' field"
        );
    }

    #[actix_web::test]
    async fn create_order_with_invalid_unit_price_returns_500() {
        let svc = make_service(InMemoryOrderRepo::default());
        let app = actix_test::init_service(
            App::new()
                .app_data(svc)
                .route("/orders", web::post().to(create_order::<InMemoryOrderRepo>)),
        )
        .await;

        let req = actix_test::TestRequest::post()
            .uri("/orders")
            .set_json(serde_json::json!({
                "customer_id": Uuid::new_v4(),
                "lines": [{"product_id": Uuid::new_v4(), "quantity": 1, "unit_price": "not-a-number"}]
            }))
            .to_request();

        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(
            resp.status(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "invalid unit_price should yield 500"
        );
    }

    #[actix_web::test]
    async fn create_order_returns_500_on_repo_internal_error() {
        let repo = InMemoryOrderRepo {
            create_error: Some("db unavailable".to_string()),
            ..Default::default()
        };
        let svc = make_service(repo);
        let app = actix_test::init_service(
            App::new()
                .app_data(svc)
                .route("/orders", web::post().to(create_order::<InMemoryOrderRepo>)),
        )
        .await;

        let req = actix_test::TestRequest::post()
            .uri("/orders")
            .set_json(serde_json::json!({
                "customer_id": Uuid::new_v4(),
                "lines": [{"product_id": Uuid::new_v4(), "quantity": 1, "unit_price": "5.00"}]
            }))
            .to_request();

        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(
            resp.status(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "repo Internal error should propagate as 500"
        );
    }

    #[actix_web::test]
    async fn get_order_returns_404_for_unknown_id() {
        let svc = make_service(InMemoryOrderRepo::default()); // find_result = None
        let app = actix_test::init_service(App::new().app_data(svc).route(
            "/orders/{id}",
            web::get().to(get_order::<InMemoryOrderRepo>),
        ))
        .await;

        let req = actix_test::TestRequest::get()
            .uri(&format!("/orders/{}", Uuid::new_v4()))
            .to_request();

        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "unknown order id should yield 404"
        );
    }

    #[actix_web::test]
    async fn get_order_returns_200_with_lines() {
        let order_id = Uuid::new_v4();
        let customer_id = Uuid::new_v4();
        let product_id = Uuid::new_v4();
        let line_id = Uuid::new_v4();

        let repo = InMemoryOrderRepo {
            find_result: Some(OrderView {
                id: order_id,
                customer_id,
                status: "PENDING".to_string(),
                created_at: Utc::now(),
                lines: vec![OrderLineView {
                    id: line_id,
                    product_id,
                    quantity: 2,
                    unit_price: BigDecimal::from_str("9.99").expect("valid decimal"),
                }],
            }),
            ..Default::default()
        };

        let svc = make_service(repo);
        let app = actix_test::init_service(App::new().app_data(svc).route(
            "/orders/{id}",
            web::get().to(get_order::<InMemoryOrderRepo>),
        ))
        .await;

        let req = actix_test::TestRequest::get()
            .uri(&format!("/orders/{}", order_id))
            .to_request();

        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "existing order should yield 200"
        );
        let body: serde_json::Value = actix_test::read_body_json(resp).await;
        assert_eq!(
            body["id"].as_str(),
            Some(order_id.to_string().as_str()),
            "response id must match"
        );
        let lines = body["lines"].as_array().expect("lines must be an array");
        assert_eq!(lines.len(), 1, "expected one order line");
        assert_eq!(lines[0]["quantity"].as_i64(), Some(2));
        assert_eq!(lines[0]["unit_price"].as_str(), Some("9.99"));
    }

    #[actix_web::test]
    async fn list_orders_returns_200_with_pagination_envelope() {
        let svc = make_service(InMemoryOrderRepo::default());
        let app = actix_test::init_service(
            App::new()
                .app_data(svc)
                .route("/orders", web::get().to(list_orders::<InMemoryOrderRepo>)),
        )
        .await;

        let req = actix_test::TestRequest::get()
            .uri("/orders?page=1&limit=10")
            .to_request();

        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "list orders should yield 200"
        );
        let body: serde_json::Value = actix_test::read_body_json(resp).await;
        assert!(
            body["items"].is_array(),
            "response must contain items array"
        );
        assert!(body["total"].is_number(), "response must contain total");
        assert_eq!(body["page"].as_i64(), Some(1), "page must be echoed back");
        assert_eq!(
            body["limit"].as_i64(),
            Some(10),
            "limit must be echoed back"
        );
    }
}
