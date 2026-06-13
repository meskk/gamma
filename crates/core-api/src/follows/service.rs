//! Follow business logic: reject self-follows, and turn a missing user (FK
//! violation) into a 400 rather than a 500.

use crate::error::ApiError;
use crate::follows::model::Follow;
use crate::follows::repository::FollowRepository;
use db::PgPool;

#[derive(Clone)]
pub struct FollowService {
    repo: FollowRepository,
}

impl FollowService {
    pub fn new(pool: PgPool) -> Self {
        Self {
            repo: FollowRepository::new(pool),
        }
    }

    pub async fn follow(&self, follower: i64, followee: i64) -> Result<(), ApiError> {
        if follower == followee {
            return Err(ApiError::Validation("self_follow"));
        }
        self.repo.follow(follower, followee).await.map_err(map_fk)
    }

    pub async fn unfollow(&self, follower: i64, followee: i64) -> Result<(), ApiError> {
        Ok(self.repo.unfollow(follower, followee).await?)
    }

    pub async fn list_following(&self, follower: i64) -> Result<Vec<Follow>, ApiError> {
        Ok(self.repo.list_following(follower).await?)
    }
}

/// Following a non-existent account hits the FK constraint — a client error.
fn map_fk(err: sqlx::Error) -> ApiError {
    if let Some(db_err) = err.as_database_error() {
        if matches!(db_err.kind(), sqlx::error::ErrorKind::ForeignKeyViolation) {
            return ApiError::Validation("unknown_user");
        }
    }
    ApiError::Database(err)
}
