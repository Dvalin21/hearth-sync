use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("not found")]
    NotFound,
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden")]
    Forbidden,
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("payload too large")]
    PayloadTooLarge,
    #[error("internal error: {0}")]
    Internal(String),
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, msg) = match &self {
            AppError::NotFound => (StatusCode::NOT_FOUND, "not found"),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            AppError::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            AppError::BadRequest(m) => (StatusCode::BAD_REQUEST, m.as_str()),
            AppError::Conflict(m) => (StatusCode::CONFLICT, m.as_str()),
            AppError::PayloadTooLarge => (StatusCode::PAYLOAD_TOO_LARGE, "payload too large"),
            AppError::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, m.as_str()),
            AppError::Db(_) | AppError::Io(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal error"),
        };
        let body = json!({ "error": msg });
        (status, axum::Json(body)).into_response()
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
