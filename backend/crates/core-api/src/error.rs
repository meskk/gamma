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
    /// The request was well-formed but invalid (e.g. empty body, unknown author)
    /// → 400. Carries a stable machine-readable code.
    Validation(&'static str),
    /// The resource requires a paid unlock the caller doesn't have → 402.
    PaymentRequired,
    /// Missing or invalid authentication → 401.
    Unauthorized,
    /// Authenticated but lacking the required role/permission → 403.
    Forbidden,
    /// A conflicting resource already exists (e.g. email taken) → 409.
    Conflict(&'static str),
    /// Too many attempts (rate limit / login throttle) → 429. Carries the wait in
    /// seconds, surfaced as a `Retry-After` header so clients can render a countdown.
    TooManyRequests { retry_after_secs: u64 },
    /// A database operation failed → 500 (details logged, not leaked to clients).
    Database(sqlx::Error),
    /// Any other internal failure → 500. Message is logged, not returned.
    Internal(String),
}

impl ApiError {
    /// Map a foreign-key violation to a caller-chosen error (e.g. `NotFound` or a
    /// `Validation` code), and any other database failure to `Database`. The one
    /// place that knows how to recognise an FK violation, so repositories don't
    /// each re-implement it.
    pub fn on_fk_violation(err: sqlx::Error, on_fk: ApiError) -> ApiError {
        if err
            .as_database_error()
            .map(|e| matches!(e.kind(), sqlx::error::ErrorKind::ForeignKeyViolation))
            .unwrap_or(false)
        {
            on_fk
        } else {
            ApiError::Database(err)
        }
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        ApiError::Database(err)
    }
}

impl From<ledger::LedgerError> for ApiError {
    fn from(err: ledger::LedgerError) -> Self {
        ApiError::Internal(err.to_string())
    }
}

impl From<settlement::SettlementError> for ApiError {
    fn from(err: settlement::SettlementError) -> Self {
        ApiError::Internal(err.to_string())
    }
}

impl From<storage::StorageError> for ApiError {
    fn from(err: storage::StorageError) -> Self {
        ApiError::Internal(err.to_string())
    }
}

#[derive(Serialize)]
struct ErrorBody {
    error: &'static str,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, retry_after_secs) = match self {
            ApiError::NotFound => (StatusCode::NOT_FOUND, "not_found", None),
            ApiError::Validation(code) => (StatusCode::BAD_REQUEST, code, None),
            ApiError::PaymentRequired => (StatusCode::PAYMENT_REQUIRED, "payment_required", None),
            ApiError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized", None),
            ApiError::Forbidden => (StatusCode::FORBIDDEN, "forbidden", None),
            ApiError::Conflict(code) => (StatusCode::CONFLICT, code, None),
            ApiError::TooManyRequests { retry_after_secs } => (
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                Some(retry_after_secs),
            ),
            ApiError::Database(err) => {
                // Log the real error; return an opaque code so we never leak SQL.
                tracing::error!(%err, "database error");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal_error", None)
            }
            ApiError::Internal(msg) => {
                tracing::error!(%msg, "internal error");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal_error", None)
            }
        };
        let mut resp = (status, Json(ErrorBody { error: code })).into_response();
        if let Some(secs) = retry_after_secs {
            resp.headers_mut().insert(
                axum::http::header::RETRY_AFTER,
                axum::http::HeaderValue::from(secs),
            );
        }
        resp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn too_many_requests_maps_to_429_with_retry_after() {
        let resp = ApiError::TooManyRequests {
            retry_after_secs: 90,
        }
        .into_response();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(resp.headers().get("retry-after").unwrap(), "90");
    }
}
