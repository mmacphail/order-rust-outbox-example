use uuid::Uuid;

use crate::domain::errors::DomainError;
use crate::domain::order::{ListResult, OrderLineInput, OrderView};
use crate::domain::ports::OrderRepository;

pub struct OrderService<R> {
    repo: R,
}

impl<R: OrderRepository> OrderService<R> {
    pub fn new(repo: R) -> Self {
        Self { repo }
    }

    pub fn create_order(
        &self,
        customer_id: Uuid,
        lines: Vec<OrderLineInput>,
    ) -> Result<Uuid, DomainError> {
        self.repo.create(customer_id, lines)
    }

    pub fn get_order(&self, id: Uuid) -> Result<Option<OrderView>, DomainError> {
        self.repo.find_by_id(id)
    }

    pub fn list_orders(&self, page: i64, limit: i64) -> Result<ListResult, DomainError> {
        self.repo.list(page, limit)
    }
}
