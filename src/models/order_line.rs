use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::schema::order_lines;

#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Identifiable, Associations)]
#[diesel(table_name = order_lines)]
#[diesel(belongs_to(crate::models::order::Order))]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct OrderLine {
    pub id: Uuid,
    pub order_id: Uuid,
    pub product_id: Uuid,
    pub quantity: i32,
    pub unit_price: BigDecimal,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = order_lines)]
pub struct NewOrderLine {
    pub id: Uuid,
    pub order_id: Uuid,
    pub product_id: Uuid,
    pub quantity: i32,
    pub unit_price: BigDecimal,
}
