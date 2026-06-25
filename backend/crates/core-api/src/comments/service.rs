//! Comment business logic: validate input, and map a comment on a non-existent post
//! to a 404.

use crate::comments::model::Comment;
use crate::comments::repository::CommentRepository;
use crate::error::ApiError;
use db::PgPool;

/// Hard cap on a comment's length.
const MAX_COMMENT_LEN: usize = 2000;

#[derive(Clone)]
pub struct CommentService {
    repo: CommentRepository,
}

impl CommentService {
    pub fn new(pool: PgPool) -> Self {
        Self {
            repo: CommentRepository::new(pool),
        }
    }

    pub async fn create(
        &self,
        post_id: i64,
        author_id: i64,
        body: String,
    ) -> Result<Comment, ApiError> {
        let body = body.trim();
        if body.is_empty() {
            return Err(ApiError::Validation("empty_comment"));
        }
        if body.len() > MAX_COMMENT_LEN {
            return Err(ApiError::Validation("comment_too_long"));
        }
        self.repo
            .create(post_id, author_id, body)
            .await
            .map_err(map_fk)
    }

    pub async fn list(&self, post_id: i64) -> Result<Vec<Comment>, ApiError> {
        Ok(self.repo.list_for_post(post_id).await?)
    }
}

/// A comment on a non-existent post hits the FK — a client error (404), not a fault.
fn map_fk(err: sqlx::Error) -> ApiError {
    if let Some(db_err) = err.as_database_error() {
        if matches!(db_err.kind(), sqlx::error::ErrorKind::ForeignKeyViolation) {
            return ApiError::NotFound;
        }
    }
    ApiError::Database(err)
}
