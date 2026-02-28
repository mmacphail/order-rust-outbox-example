use actix_web::HttpResponse;
use thiserror::Error;

use crate::domain::errors::DomainError;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Not found")]
    NotFound,

    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<DomainError> for AppError {
    fn from(e: DomainError) -> Self {
        match e {
            DomainError::NotFound => AppError::NotFound,
            DomainError::InvalidInput(msg) | DomainError::Internal(msg) => AppError::Internal(msg),
        }
    }
}

impl actix_web::ResponseError for AppError {
    fn error_response(&self) -> HttpResponse {
        match self {
            AppError::NotFound => HttpResponse::NotFound().json(serde_json::json!({
                "error": self.to_string()
            })),
            AppError::Internal(_) => HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Internal server error"
            })),
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
    fn domain_not_found_maps_to_app_not_found() {
        let app_err: AppError = DomainError::NotFound.into();
        assert!(matches!(app_err, AppError::NotFound));
    }

    #[test]
    fn domain_internal_maps_to_app_internal() {
        let app_err: AppError = DomainError::Internal("oops".to_string()).into();
        assert!(matches!(app_err, AppError::Internal(_)));
    }

    #[test]
    fn domain_invalid_input_maps_to_app_internal() {
        let app_err: AppError = DomainError::InvalidInput("bad value".to_string()).into();
        assert!(matches!(app_err, AppError::Internal(_)));
    }
}
