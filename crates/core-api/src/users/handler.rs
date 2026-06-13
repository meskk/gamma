//! HTTP layer for users: translate requests into service calls and back.
//! No business logic, no SQL — those live in `service` and `repository`.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::error::ApiError;
use crate::state::AppState;
use crate::users::model::{NewUser, User};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/users", post(create_user))
        .route("/users/:id", get(get_user))
}

async fn create_user(
    State(state): State<AppState>,
    Json(body): Json<NewUser>,
) -> Result<(StatusCode, Json<User>), ApiError> {
    let user = state.users.create(body).await?;
    Ok((StatusCode::CREATED, Json(user)))
}

async fn get_user(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<User>, ApiError> {
    let user = state.users.get(id).await?;
    Ok(Json(user))
}
