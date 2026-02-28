use thiserror::Error;

#[derive(Debug, Error)]
pub enum DomainError {
    #[error("Order not found")]
    NotFound,
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    #[error("Internal error: {0}")]
    Internal(String),
}
