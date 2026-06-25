//! Comment business logic: validate input, and map a comment on a missing OR
//! hidden (taken-down) post to a 404.

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
        // `None` ⇒ the post is missing OR hidden (taken down) ⇒ 404. A non-existent
        // author still trips the FK, which `map_fk` also maps to a 404.
        match self.repo.create(post_id, author_id, body).await {
            Ok(Some(comment)) => Ok(comment),
            Ok(None) => Err(ApiError::NotFound),
            Err(err) => Err(map_fk(err)),
        }
    }

    pub async fn list(&self, post_id: i64) -> Result<Vec<Comment>, ApiError> {
        Ok(self.repo.list_for_post(post_id).await?)
    }
}

/// A comment with a non-existent author hits the FK — a client error (404), not a
/// fault. (A missing/hidden post is already handled by the `None` arm above.)
fn map_fk(err: sqlx::Error) -> ApiError {
    ApiError::on_fk_violation(err, ApiError::NotFound)
}
