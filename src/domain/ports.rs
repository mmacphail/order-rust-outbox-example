use uuid::Uuid;

use super::errors::DomainError;
use super::order::{ListResult, OrderLineInput, OrderView};

pub trait OrderRepository: Send + Sync + 'static {
    fn create(&self, customer_id: Uuid, lines: Vec<OrderLineInput>) -> Result<Uuid, DomainError>;
    fn find_by_id(&self, id: Uuid) -> Result<Option<OrderView>, DomainError>;
    fn list(&self, page: i64, limit: i64) -> Result<ListResult, DomainError>;
}
