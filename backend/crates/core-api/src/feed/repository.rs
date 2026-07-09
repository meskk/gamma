//! Feed candidate retrieval — the bounded candidate-set query (Dossier App. A.2).

use crate::posts::model::Post;
use db::PgPool;

#[derive(Clone)]
pub struct FeedRepository {
    pool: PgPool,
}

impl FeedRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Bounded candidate set: posts from followed authors, from the viewer's
    /// declared categories, and globally popular — each source LIMIT-capped
    /// before the UNION, all within a 48h recency window. Worst case ~2,000 rows,
    /// so latency stays bounded no matter how many accounts the viewer follows.
    ///
    /// The 48h window is a query-time literal bound (NOT a partial index, since
    /// `now()` is STABLE not IMMUTABLE — App. A.2). The `"col!"` aliases override
    /// sqlx's nullable inference through the UNION for the genuinely non-null
    /// columns; `category`/`body` stay nullable to match `Post`.
    ///
    /// Each CTE also applies the private-area visibility predicate (P-4/A4,
    /// ADR 0011 §5) with `$1` (the feed's own `user_id`) as the viewer: a private
    /// post reaches the feed only if the viewer is its author, holds a live
    /// entitlement, or the creator's area is `free`. The viewer is always a
    /// logged-in user here, so the free arm needs no `IS NOT NULL` guard (unlike
    /// the anonymous-capable get/list predicate).
    pub async fn candidates(
        &self,
        user_id: i64,
        categories: &[String],
    ) -> Result<Vec<Post>, sqlx::Error> {
        sqlx::query_as!(
            Post,
            r#"
            WITH cutoff AS (SELECT now() - interval '48 hours' AS t),
            from_follows AS (
                SELECT p.id, p.author_id, p.category, p.body, p.created_at, p.popularity_score, p.media_id, p.area
                FROM posts p
                JOIN follows f ON f.followee_id = p.author_id
                WHERE f.follower_id = $1 AND p.created_at > (SELECT t FROM cutoff)
                  AND p.hidden_at IS NULL
                  AND (
                    p.area = 'public'
                    OR p.author_id = $1
                    OR EXISTS (SELECT 1 FROM area_entitlements ae WHERE ae.viewer_id = $1 AND ae.creator_id = p.author_id AND (ae.expires_at IS NULL OR ae.expires_at > now()))
                    OR EXISTS (SELECT 1 FROM private_areas pa WHERE pa.creator_id = p.author_id AND pa.access_model = 'free')
                  )
                ORDER BY p.created_at DESC
                LIMIT 800
            ),
            from_category AS (
                SELECT p.id, p.author_id, p.category, p.body, p.created_at, p.popularity_score, p.media_id, p.area
                FROM posts p
                WHERE p.category = ANY($2) AND p.created_at > (SELECT t FROM cutoff)
                  AND p.hidden_at IS NULL
                  AND (
                    p.area = 'public'
                    OR p.author_id = $1
                    OR EXISTS (SELECT 1 FROM area_entitlements ae WHERE ae.viewer_id = $1 AND ae.creator_id = p.author_id AND (ae.expires_at IS NULL OR ae.expires_at > now()))
                    OR EXISTS (SELECT 1 FROM private_areas pa WHERE pa.creator_id = p.author_id AND pa.access_model = 'free')
                  )
                ORDER BY p.created_at DESC
                LIMIT 800
            ),
            from_popularity AS (
                SELECT p.id, p.author_id, p.category, p.body, p.created_at, p.popularity_score, p.media_id, p.area
                FROM posts p
                WHERE p.created_at > (SELECT t FROM cutoff)
                  AND p.hidden_at IS NULL
                  AND (
                    p.area = 'public'
                    OR p.author_id = $1
                    OR EXISTS (SELECT 1 FROM area_entitlements ae WHERE ae.viewer_id = $1 AND ae.creator_id = p.author_id AND (ae.expires_at IS NULL OR ae.expires_at > now()))
                    OR EXISTS (SELECT 1 FROM private_areas pa WHERE pa.creator_id = p.author_id AND pa.access_model = 'free')
                  )
                ORDER BY p.popularity_score DESC
                LIMIT 400
            ),
            candidates AS (
                SELECT id, author_id, category, body, created_at, popularity_score, media_id, area FROM from_follows
                UNION
                SELECT id, author_id, category, body, created_at, popularity_score, media_id, area FROM from_category
                UNION
                SELECT id, author_id, category, body, created_at, popularity_score, media_id, area FROM from_popularity
            )
            SELECT
                id AS "id!",
                author_id AS "author_id!",
                category,
                body,
                created_at AS "created_at!",
                popularity_score AS "popularity_score!",
                media_id,
                area AS "area!"
            FROM candidates
            "#,
            user_id,
            categories
        )
        .fetch_all(&self.pool)
        .await
    }
}
