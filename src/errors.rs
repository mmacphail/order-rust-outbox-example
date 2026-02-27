use actix_web::HttpResponse;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Not found")]
    NotFound,

    #[error("Database error: {0}")]
    DatabaseError(#[from] diesel::result::Error),

    #[error("Connection pool error: {0}")]
    PoolError(#[from] r2d2::Error),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl actix_web::ResponseError for AppError {
    fn error_response(&self) -> HttpResponse {
        match self {
            AppError::NotFound => HttpResponse::NotFound().json(serde_json::json!({
                "error": self.to_string()
            })),
            AppError::DatabaseError(_) | AppError::PoolError(_) | AppError::Internal(_) => {
                HttpResponse::InternalServerError().json(serde_json::json!({
                    "error": "Internal server error"
                }))
            }
        }
    }
}
