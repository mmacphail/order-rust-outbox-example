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

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::ResponseError;

    #[test]
    fn not_found_returns_404() {
        let resp = AppError::NotFound.error_response();
        assert_eq!(resp.status(), actix_web::http::StatusCode::NOT_FOUND);
    }

    #[test]
    fn database_error_returns_500() {
        let err = AppError::DatabaseError(diesel::result::Error::NotFound);
        assert_eq!(
            err.error_response().status(),
            actix_web::http::StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn internal_error_returns_500() {
        let err = AppError::Internal("something went wrong".to_string());
        assert_eq!(
            err.error_response().status(),
            actix_web::http::StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn not_found_display() {
        assert_eq!(AppError::NotFound.to_string(), "Not found");
    }

    #[test]
    fn internal_error_display() {
        assert_eq!(
            AppError::Internal("msg".to_string()).to_string(),
            "Internal error: msg"
        );
    }

    #[test]
    fn database_error_display() {
        let err = AppError::DatabaseError(diesel::result::Error::NotFound);
        assert!(
            err.to_string().starts_with("Database error:"),
            "expected display to start with 'Database error:', got: {}",
            err
        );
    }

    #[test]
    fn from_diesel_not_found_wraps_as_database_error() {
        let app_err: AppError = diesel::result::Error::NotFound.into();
        assert!(matches!(app_err, AppError::DatabaseError(_)));
    }

    #[test]
    fn from_diesel_database_error_converts() {
        let app_err: AppError = diesel::result::Error::RollbackTransaction.into();
        assert!(matches!(app_err, AppError::DatabaseError(_)));
    }
}
