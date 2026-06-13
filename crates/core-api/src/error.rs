//! Single API error type. Every handler returns `Result<_, ApiError>`; this maps
//! domain and database failures to HTTP status codes in ONE place, so error
//! handling is uniform across every route.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

#[derive(Debug)]
pub enum ApiError {
    /// The requested resource does not exist → 404.
    NotFound,
    /// A database operation failed → 500 (details logged, not leaked to clients).
    Database(sqlx::Error),
}

impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        ApiError::Database(err)
    }
}

#[derive(Serialize)]
struct ErrorBody {
    error: &'static str,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = match self {
            ApiError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            ApiError::Database(err) => {
                // Log the real error; return an opaque code so we never leak SQL.
                tracing::error!(%err, "database error");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
            }
        };
        (status, Json(ErrorBody { error: code })).into_response()
    }
}
