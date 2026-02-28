use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct OrderLineInput {
    pub product_id: Uuid,
    pub quantity: i32,
    pub unit_price: BigDecimal,
}

#[derive(Debug, Clone)]
pub struct OrderLineView {
    pub id: Uuid,
    pub product_id: Uuid,
    pub quantity: i32,
    pub unit_price: BigDecimal,
}

#[derive(Debug, Clone)]
pub struct OrderView {
    pub id: Uuid,
    pub customer_id: Uuid,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub lines: Vec<OrderLineView>,
}

#[derive(Debug, Clone)]
pub struct ListResult {
    pub items: Vec<OrderView>,
    pub total: i64,
}
