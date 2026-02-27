use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::schema::orders;

#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Identifiable)]
#[diesel(table_name = orders)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Order {
    pub id: Uuid,
    pub customer_id: Uuid,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = orders)]
pub struct NewOrder {
    pub id: Uuid,
    pub customer_id: Uuid,
    pub status: String,
}
