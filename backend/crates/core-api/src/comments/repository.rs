//! Postgres persistence for comments — the only place that knows comments SQL.

use crate::comments::model::Comment;
use crate::interactions::model::InteractionType;
use db::PgPool;

#[derive(Clone)]
pub struct CommentRepository {
    pool: PgPool,
}

impl CommentRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Insert a comment — but only when the target post exists, is visible (not
    /// taken down), AND the author (`$2`) may SEE it (the area predicate, P-4/A4).
    /// Returns `None` when the post is missing, hidden, or PRIVATE-and-unseen,
    /// which the service surfaces as a 404 — so a non-entitled user can't confirm
    /// a private post exists by probing whether a comment succeeds. The commenter
    /// is always authenticated, so the free arm needs no `IS NOT NULL` guard.
    pub async fn create(
        &self,
        post_id: i64,
        author_id: i64,
        body: &str,
    ) -> Result<Option<Comment>, sqlx::Error> {
        sqlx::query_as!(
            Comment,
            r#"
            INSERT INTO comments (post_id, author_id, body)
            SELECT $1, $2, $3
            WHERE EXISTS (
                SELECT 1 FROM posts WHERE id = $1 AND hidden_at IS NULL
                  AND (
                    area = 'public'
                    OR author_id = $2
                    OR EXISTS (SELECT 1 FROM area_entitlements ae WHERE ae.viewer_id = $2 AND ae.creator_id = posts.author_id AND (ae.expires_at IS NULL OR ae.expires_at > now()))
                    OR EXISTS (SELECT 1 FROM private_areas pa WHERE pa.creator_id = posts.author_id AND pa.access_model = 'free')
                  )
            )
            RETURNING id, post_id, author_id, body, created_at,
                      0::bigint AS "like_count!", false AS "liked_by_me!"
            "#,
            post_id,
            author_id,
            body
        )
        .fetch_optional(&self.pool)
        .await
    }

    /// Can `viewer` SEE the comment — i.e. its POST (a comment is exactly as
    /// visible as its post)? The write-side A4f guard for comment-directed
    /// interactions, as ONE query: "comment missing" and "comment on an unseen
    /// private post" are indistinguishable in result AND in work done (a
    /// two-step resolve-then-check would answer a missing comment after one
    /// query but a hidden one after two — a timing tell). Mirrors
    /// `PostRepository::post_visible_to`: the area predicate only, no
    /// `hidden_at` (interactions on hidden posts stay write-accepted and
    /// economically inert — see the posts module invariant doc). The caller is
    /// always authenticated, so the free arm keeps the `IS NOT NULL` guard only
    /// for symmetry with the anonymous-capable predicate.
    pub async fn comment_visible_to(
        &self,
        comment_id: i64,
        viewer: Option<i64>,
    ) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar!(
            r#"
            SELECT EXISTS (
                SELECT 1 FROM comments c
                JOIN posts p ON p.id = c.post_id
                WHERE c.id = $1
                  AND (
                    p.area = 'public'
                    OR p.author_id = $2
                    OR EXISTS (SELECT 1 FROM area_entitlements ae WHERE ae.viewer_id = $2 AND ae.creator_id = p.author_id AND (ae.expires_at IS NULL OR ae.expires_at > now()))
                    OR EXISTS (SELECT 1 FROM private_areas pa WHERE pa.creator_id = p.author_id AND pa.access_model = 'free' AND $2::bigint IS NOT NULL)
                  )
            ) AS "visible!"
            "#,
            comment_id,
            viewer
        )
        .fetch_one(&self.pool)
        .await
    }

    /// A visible post's comments, oldest first, bounded by `limit`/`offset` so a
    /// pathological thread can't return an unbounded result set. Comments on a
    /// taken-down (`hidden_at`) post are excluded — moderation hides the thread —
    /// and comments on a PRIVATE post the `viewer` isn't entitled to are excluded
    /// too (the area predicate). `viewer` is `None` for an anonymous caller: a
    /// private post's thread then returns an empty list (NOT 404 — must not diverge
    /// from a public post that simply has no comments).
    pub async fn list_for_post(
        &self,
        post_id: i64,
        viewer: Option<i64>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Comment>, sqlx::Error> {
        sqlx::query_as!(
            Comment,
            r#"
            SELECT c.id, c.post_id, c.author_id, c.body, c.created_at,
                   (SELECT COUNT(DISTINCT ie.actor_id) FROM interaction_events ie
                    WHERE ie.comment_id = c.id AND ie.type = $5 AND ie.retracted_at IS NULL)
                       AS "like_count!",
                   EXISTS(SELECT 1 FROM interaction_events ie
                    WHERE ie.comment_id = c.id AND ie.type = $5 AND ie.actor_id = $2
                      AND ie.retracted_at IS NULL) AS "liked_by_me!"
            FROM comments c
            JOIN posts p ON p.id = c.post_id
            WHERE c.post_id = $1 AND p.hidden_at IS NULL
              AND (
                p.area = 'public'
                OR p.author_id = $2
                OR EXISTS (SELECT 1 FROM area_entitlements ae WHERE ae.viewer_id = $2 AND ae.creator_id = p.author_id AND (ae.expires_at IS NULL OR ae.expires_at > now()))
                OR EXISTS (SELECT 1 FROM private_areas pa WHERE pa.creator_id = p.author_id AND pa.access_model = 'free' AND $2::bigint IS NOT NULL)
              )
            ORDER BY c.created_at ASC
            LIMIT $3 OFFSET $4
            "#,
            post_id,
            viewer,
            limit,
            offset,
            InteractionType::Like.code()
        )
        .fetch_all(&self.pool)
        .await
    }
}
