//! Postgres-backed post repository — the only place that knows posts SQL.
//! Same shape as the users repository (concrete struct, `query_as!` checked
//! queries). Adds `list` to show the multi-row (`fetch_all`) template.
//!
//! ## Post-visibility invariants — READ THIS BEFORE ADDING A QUERY
//!
//! TWO orthogonal invariants gate every read of `posts` (here OR via a
//! `JOIN`/subquery from another domain). sqlx's compile-time macros can't share a
//! `WHERE` fragment, so both are enforced per query; a forgotten filter OVER-hides
//! (safe), a mistyped one is the only leak risk and is locked per surface by a test.
//!
//! ### 1. Moderation (`hidden_at`)
//! A taken-down post has `hidden_at` set (operator action). Every user-facing read
//! MUST exclude `hidden_at IS NOT NULL`, and every write *attached to* a post MUST
//! refuse a hidden one. This invariant has regressed twice (comments, interactions).
//!
//! ### 2. Area / private-area entitlement (`area`, P-4/A4, ADR 0011 §5)
//! A post with `area = 'private'` is the creator's paywalled area and must surface
//! in NO read path unless the viewer is entitled. The viewer-scoped predicate (a
//! separate conjunct, adjacent to `hidden_at`, NOT merged with it) is, with `p` the
//! `posts` alias and `$V` the viewer bound as `Option<i64>` (NULL = anonymous):
//!
//! ```sql
//! (   p.area = 'public'
//!  OR p.author_id = $V
//!  OR EXISTS (SELECT 1 FROM area_entitlements ae
//!             WHERE ae.viewer_id = $V AND ae.creator_id = p.author_id
//!               AND (ae.expires_at IS NULL OR ae.expires_at > now()))
//!  OR EXISTS (SELECT 1 FROM private_areas pa
//!             WHERE pa.creator_id = p.author_id AND pa.access_model = 'free'
//!               AND $V IS NOT NULL) )   -- free = members, login required (owner-decided 2026-07-09)
//! ```
//!
//! This is the SQL form of `PrivateAreaRepository::is_entitled` EXTENDED with the
//! creator arm and the `access_model = 'free'` arm it deliberately omits. NOTE a
//! consequence of the uniform predicate: a `free` area's private posts are visible
//! to ANY logged-in viewer wherever posts are read — including the GLOBAL timeline
//! (`GET /posts` with no author filter), not only the creator's profile. That is
//! consistent with "a free area's members (= any logged-in user) are entitled"
//! (owner-decided 2026-07-09: free = login required, not world-readable). It gates
//! per-CREATOR: one entitlement row grants all of a creator's private posts (correct
//! for free/one_time/subscription; `per_post` read-gating is DEFERRED to its payment
//! stage, A9). The guard is INDEPENDENT of the `GAMMA_PRIVATE_AREA` flag — that flag
//! only gates the A3 config routes; a private post can exist and must never leak.
//! Producer/economic rails (settlement, ingestion) use the blanket `p.area = 'public'`
//! (no viewer) — private posts leave those rails unconditionally (Rail-1/Rail-2
//! separation; private posts are never analysed). The ingestion producer is gated
//! at BOTH ends: `PostService::create` skips the enqueue for `area != 'public'`
//! (A4g, the create-time producer), and the backfill producer filters the same
//! (A4a). As a backstop, the AI worker fetches each post through the anonymous
//! `get_post` API read (ADR 0006), which the A4b area predicate 404s for a private
//! post — so even a private post that somehow reached the queue yields no
//! `content_signals` row. That backstop makes the ORDERING load-bearing: A4b (the
//! read gate) had to land before A4g (the write path that can first mint a private
//! post); the plan ordered them so.
//!
//! Surfaces that MUST filter `hidden_at` (all do):
//!   - `get` / `list` (here) · the three feed CTEs (`feed::repository::candidates`)
//!   - comment read + write (`comments::repository`)
//!   - settlement edges (`interactions::repository::edges_for_epoch`) — drops the
//!     gem-weight of likes on hidden posts, including likes recorded before takedown
//!   - ingestion backfill / status (`unanalyzed_post_ids`, `count_unanalyzed_posts`,
//!     `signals_count_by_model_version`, `count_embeddings`)
//!
//! Surfaces that MUST apply the AREA predicate/gate (A4b–A4f wire the viewer):
//!   - `get` / `list` (viewer-scoped) · the three feed CTEs · comment read + write
//!   - the MEDIA rail (out of this crate: `media::service` gates an asset through its
//!     owning `posts.media_id` join — the one path `hidden_at` structurally misses)
//!   - the write-side existence oracles (`report`, `interactions::record`) via
//!     `post_visible_to`
//!   - settlement + ingestion use the blanket `p.area = 'public'` (no viewer)
//!
//! Deliberate exceptions:
//!   - operator surfaces (`list_reported`) intentionally include BOTH hidden and
//!     private rows (ADR 0011 §5 116-117); `ReportedPost.area` surfaces which
//!   - the interaction *write* path ACCEPTS a like on a hidden post at insert time
//!     (only the AREA predicate is service-checked in A4f, NOT `hidden_at`); the
//!     like is economically inert because `edges_for_epoch` — the authoritative
//!     guard — drops it at settlement. So hidden-post interactions are not
//!     re-guarded at insert; do not add a `hidden_at` insert check expecting a 404
//!
//! FUTURE: an M2.7 `content_signals`/embedding-driven ranker is a NEW post-content
//! read path — it MUST re-apply the area predicate, OR stale signal/embedding rows
//! must be purged when a post flips public->private.

use chrono::{DateTime, Utc};

use crate::posts::model::{NewPost, Post, ReportedPost};
use db::PgPool;

#[derive(Clone)]
pub struct PostRepository {
    pool: PgPool,
}

impl PostRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, new: &NewPost) -> Result<Post, sqlx::Error> {
        sqlx::query_as!(
            Post,
            r#"
            INSERT INTO posts (author_id, category, body, media_id, area)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id, author_id, category, body, created_at, popularity_score, media_id, area
            "#,
            new.author_id,
            new.category.as_deref(),
            new.body,
            new.media_id,
            new.area
        )
        .fetch_one(&self.pool)
        .await
    }

    /// Whether `media_id` exists AND is owned by `owner_id`. Lets the service
    /// pre-validate an attached asset so a missing/not-owned media id is reported
    /// precisely (`unknown_media`) instead of tripping the post's media FK and
    /// being misread as a bad author.
    pub async fn media_owned_by(&self, media_id: i64, owner_id: i64) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar!(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM media_assets WHERE id = $1 AND owner_id = $2
            ) AS "exists!"
            "#,
            media_id,
            owner_id
        )
        .fetch_one(&self.pool)
        .await
    }

    /// Can `viewer` SEE post `post_id` (the area predicate only — moderation
    /// `hidden_at` is deliberately not applied here, so this does not change the
    /// existing report/interaction behaviour on hidden posts)? The write-side
    /// existence guard for the report and interaction paths (A4f): a viewer who
    /// can't see a private post can neither report it nor interact with it, and
    /// gets the same `NotFound` as if the post didn't exist — closing the
    /// success-vs-404 oracle. `viewer` is `None` for anonymous, but both callers
    /// are authenticated so it is always `Some`.
    pub async fn post_visible_to(
        &self,
        post_id: i64,
        viewer: Option<i64>,
    ) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar!(
            r#"
            SELECT EXISTS (
                SELECT 1 FROM posts p
                WHERE p.id = $1
                  AND (
                    p.area = 'public'
                    OR p.author_id = $2
                    OR EXISTS (SELECT 1 FROM area_entitlements ae WHERE ae.viewer_id = $2 AND ae.creator_id = p.author_id AND (ae.expires_at IS NULL OR ae.expires_at > now()))
                    OR EXISTS (SELECT 1 FROM private_areas pa WHERE pa.creator_id = p.author_id AND pa.access_model = 'free' AND $2::bigint IS NOT NULL)
                  )
            ) AS "visible!"
            "#,
            post_id,
            viewer
        )
        .fetch_one(&self.pool)
        .await
    }

    /// A single post — but not if it has been taken down (`hidden_at`), and not a
    /// PRIVATE post unless `viewer` may see it (the area predicate; see the module
    /// invariant doc). `viewer` is `None` for an anonymous caller: a private post
    /// then fails every arm and this returns `None` (→ 404, no existence oracle).
    pub async fn get(&self, id: i64, viewer: Option<i64>) -> Result<Option<Post>, sqlx::Error> {
        sqlx::query_as!(
            Post,
            r#"
            SELECT id, author_id, category, body, created_at, popularity_score, media_id, area
            FROM posts
            WHERE id = $1 AND hidden_at IS NULL
              AND (
                area = 'public'
                OR author_id = $2
                OR EXISTS (
                    SELECT 1 FROM area_entitlements ae
                    WHERE ae.viewer_id = $2 AND ae.creator_id = posts.author_id
                      AND (ae.expires_at IS NULL OR ae.expires_at > now())
                )
                OR EXISTS (
                    SELECT 1 FROM private_areas pa
                    WHERE pa.creator_id = posts.author_id AND pa.access_model = 'free'
                      AND $2::bigint IS NOT NULL
                )
              )
            "#,
            id,
            viewer
        )
        .fetch_optional(&self.pool)
        .await
    }

    /// Most recent visible posts first, paged by `limit`/`offset` (the caller
    /// clamps them). When `author_id` is `Some`, only that author's posts (the
    /// profile feed). `offset` makes older posts reachable — previously the list
    /// could only ever return the newest page.
    pub async fn list(
        &self,
        author_id: Option<i64>,
        viewer: Option<i64>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Post>, sqlx::Error> {
        sqlx::query_as!(
            Post,
            r#"
            SELECT id, author_id, category, body, created_at, popularity_score, media_id, area
            FROM posts
            WHERE hidden_at IS NULL AND ($1::bigint IS NULL OR author_id = $1)
              AND (
                area = 'public'
                OR author_id = $2
                OR EXISTS (
                    SELECT 1 FROM area_entitlements ae
                    WHERE ae.viewer_id = $2 AND ae.creator_id = posts.author_id
                      AND (ae.expires_at IS NULL OR ae.expires_at > now())
                )
                OR EXISTS (
                    SELECT 1 FROM private_areas pa
                    WHERE pa.creator_id = posts.author_id AND pa.access_model = 'free'
                      AND $2::bigint IS NOT NULL
                )
              )
            ORDER BY created_at DESC
            LIMIT $3 OFFSET $4
            "#,
            author_id,
            viewer,
            limit,
            offset
        )
        .fetch_all(&self.pool)
        .await
    }

    /// Record a report of a post. Idempotent per (post, reporter) — re-reporting
    /// is a no-op (returns `false`), not amplification. A non-existent post hits
    /// the FK and surfaces as a 404 at the service.
    pub async fn report(
        &self,
        post_id: i64,
        reporter_id: i64,
        reason: &str,
    ) -> Result<bool, sqlx::Error> {
        let res = sqlx::query!(
            r#"
            INSERT INTO post_reports (post_id, reporter_id, reason)
            VALUES ($1, $2, $3)
            ON CONFLICT (post_id, reporter_id) DO NOTHING
            "#,
            post_id,
            reporter_id,
            reason
        )
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() == 1)
    }

    /// Take down (`hidden_at = now()`) or restore (`hidden_at = NULL`) a post.
    /// Returns the row, or `None` if no such post.
    pub async fn set_hidden(
        &self,
        id: i64,
        hidden_at: Option<DateTime<Utc>>,
    ) -> Result<Option<Post>, sqlx::Error> {
        sqlx::query_as!(
            Post,
            r#"
            UPDATE posts SET hidden_at = $2
            WHERE id = $1
            RETURNING id, author_id, category, body, created_at, popularity_score, media_id, area
            "#,
            id,
            hidden_at
        )
        .fetch_optional(&self.pool)
        .await
    }

    /// Post ids with NO `content_signals` row yet (and not taken down), id-ordered,
    /// after a cursor, capped by `limit`. The backfill producer: the existing corpus
    /// is otherwise invisible to the ingestion pipeline, which only sees NEW posts.
    /// Read-only — it selects ids; it never touches the signals payload or the feed.
    pub async fn unanalyzed_post_ids(
        &self,
        after_id: i64,
        limit: i64,
    ) -> Result<Vec<i64>, sqlx::Error> {
        sqlx::query_scalar!(
            r#"
            SELECT p.id AS "id!"
            FROM posts p
            LEFT JOIN content_signals cs ON cs.post_id = p.id
            WHERE cs.post_id IS NULL
              AND p.hidden_at IS NULL
              AND p.area = 'public'
              AND p.id > $1
            ORDER BY p.id
            LIMIT $2
            "#,
            after_id,
            limit
        )
        .fetch_all(&self.pool)
        .await
    }

    /// How many visible posts have NO `content_signals` row yet — i.e. exactly what
    /// a full backfill sweep would enqueue. Read-only count for operator status.
    pub async fn count_unanalyzed_posts(&self) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar!(
            r#"
            SELECT COUNT(*) AS "count!"
            FROM posts p
            LEFT JOIN content_signals cs ON cs.post_id = p.id
            WHERE cs.post_id IS NULL AND p.hidden_at IS NULL AND p.area = 'public'
            "#
        )
        .fetch_one(&self.pool)
        .await
    }

    /// Count of analysed VISIBLE posts grouped by the `model_version` that produced
    /// them. Joins posts and filters out taken-down ones so this count partitions
    /// the same universe as `count_unanalyzed_posts` (visible posts): analysed +
    /// unanalysed = all visible posts. Read-only — counts rows, never reads payload.
    pub async fn signals_count_by_model_version(&self) -> Result<Vec<(String, i64)>, sqlx::Error> {
        let rows = sqlx::query!(
            r#"
            SELECT cs.model_version, COUNT(*) AS "count!"
            FROM content_signals cs
            JOIN posts p ON p.id = cs.post_id
            WHERE p.hidden_at IS NULL AND p.area = 'public'
            GROUP BY cs.model_version
            ORDER BY cs.model_version
            "#
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| (r.model_version, r.count))
            .collect())
    }

    /// How many posts have a stored embedding (visible posts, same universe as
    /// the other status counts). Counts rows only — never reads the vectors.
    pub async fn count_embeddings(&self) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar!(
            r#"
            SELECT COUNT(*) AS "count!"
            FROM post_embeddings pe
            JOIN posts p ON p.id = pe.post_id
            WHERE p.hidden_at IS NULL AND p.area = 'public'
            "#
        )
        .fetch_one(&self.pool)
        .await
    }

    /// Reported posts with their report counts, most-reported first (operator
    /// review queue).
    pub async fn list_reported(&self, limit: i64) -> Result<Vec<ReportedPost>, sqlx::Error> {
        sqlx::query_as!(
            ReportedPost,
            r#"
            SELECT
                p.id AS "post_id!",
                COUNT(r.id) AS "report_count!",
                (p.hidden_at IS NOT NULL) AS "hidden!",
                p.area AS "area!"
            FROM posts p
            JOIN post_reports r ON r.post_id = p.id
            GROUP BY p.id
            ORDER BY COUNT(r.id) DESC, p.id
            LIMIT $1
            "#,
            limit
        )
        .fetch_all(&self.pool)
        .await
    }
}
