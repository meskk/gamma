//! Follow business logic: reject self-follows, and turn a missing user (FK
//! violation) into a 400 rather than a 500.

use crate::error::ApiError;
use crate::follows::model::Follow;
use crate::follows::repository::FollowRepository;
use db::PgPool;

/// Default and maximum page size for the following list. The default is generous
/// (the frontend derives follow state from the list today), while the max bounds
/// the worst case a single request can return.
const DEFAULT_PAGE: i64 = 1000;
const MAX_PAGE: i64 = 1000;

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

    pub async fn list_following(
        &self,
        follower: i64,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<Vec<Follow>, ApiError> {
        let limit = limit.unwrap_or(DEFAULT_PAGE).clamp(1, MAX_PAGE);
        let offset = offset.unwrap_or(0).max(0);
        Ok(self.repo.list_following(follower, limit, offset).await?)
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
