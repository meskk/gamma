//! Feed business logic: fetch the viewer, pull the candidate set, and rank it
//! with the Phase-1 cold-start ranker (popularity + recency + category match).
//!
//! This deliberately uses no per-user ML — that is the Phase-2 replacement
//! (Dossier §4.2). The ranker is a pure, deterministic function so it is easy to
//! reason about and test.

use chrono::{DateTime, Utc};

use crate::error::ApiError;
use crate::feed::cursor::{self, FeedCursor};
use crate::feed::model::FeedPage;
use crate::feed::repository::FeedRepository;
use crate::posts::model::Post;
use crate::users::repository::UserRepository;
use db::PgPool;

/// Hard cap on feed size per request.
const MAX_FEED_LIMIT: usize = 100;
/// A cursor older than the 48h candidate window ranks over a vanished set —
/// reject it as stale and let the client refresh instead of serving weirdness.
const CURSOR_MAX_AGE_SECS: i64 = 48 * 3600;
/// Recency half-life is ~ln(2)/decay hours; 0.05/h ≈ a 14h half-life.
const RECENCY_DECAY_PER_HOUR: f64 = 0.05;
/// Flat boost for a post whose category the viewer declared.
const CATEGORY_BONUS: f64 = 1.0;

#[derive(Clone)]
pub struct FeedService {
    feed: FeedRepository,
    users: UserRepository,
}

impl FeedService {
    pub fn new(pool: PgPool) -> Self {
        Self {
            feed: FeedRepository::new(pool.clone()),
            users: UserRepository::new(pool),
        }
    }

    /// One page of the viewer's ranked feed (B1). Page one freezes the ranking
    /// clock to WHOLE seconds (`score` only sees whole-second ages, and whole
    /// seconds survive the cursor round-trip exactly); every later page
    /// re-scores the same candidate query with that frozen clock, so the order
    /// is reproducible — no duplicates, no gaps for items present at freeze
    /// time. Malformed cursors → 400 `invalid_cursor`; cursors older than the
    /// candidate window → 400 `stale_cursor` (client refreshes).
    pub async fn personalized(
        &self,
        user_id: i64,
        limit: usize,
        cursor: Option<&str>,
    ) -> Result<FeedPage, ApiError> {
        let viewer = self.users.get(user_id).await?.ok_or(ApiError::NotFound)?;

        let (ranked_at, after) = match cursor {
            None => (whole_seconds(Utc::now()), None),
            Some(raw) => {
                let cur = cursor::decode(raw).ok_or(ApiError::Validation("invalid_cursor"))?;
                let now = Utc::now().timestamp();
                // Small allowance for clock skew; anything further in the
                // future was never issued by us.
                if cur.ranked_at > now + 60 {
                    return Err(ApiError::Validation("invalid_cursor"));
                }
                if now - cur.ranked_at > CURSOR_MAX_AGE_SECS {
                    return Err(ApiError::Validation("stale_cursor"));
                }
                let ranked_at = DateTime::<Utc>::from_timestamp(cur.ranked_at, 0)
                    .ok_or(ApiError::Validation("invalid_cursor"))?;
                (
                    ranked_at,
                    Some((f64::from_bits(cur.score_bits), cur.last_id)),
                )
            }
        };

        let mut candidates = self
            .feed
            .candidates(user_id, &viewer.declared_categories)
            .await?;

        candidates.sort_by(|a, b| {
            let sa = score(a, &viewer.declared_categories, ranked_at);
            let sb = score(b, &viewer.declared_categories, ranked_at);
            // Highest score first; deterministic tie-break by newest id.
            sb.partial_cmp(&sa)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(b.id.cmp(&a.id))
        });

        // Keyset: keep only items strictly AFTER the cursor position in the
        // sort order. Scores compare bit-exact because both sides come from
        // the same pure function over the same frozen clock.
        if let Some((cursor_score, cursor_id)) = after {
            let cats = &viewer.declared_categories;
            candidates.retain(|p| {
                let s = score(p, cats, ranked_at);
                s < cursor_score || (s == cursor_score && p.id < cursor_id)
            });
        }

        let limit = limit.clamp(1, MAX_FEED_LIMIT);
        let has_more = candidates.len() > limit;
        candidates.truncate(limit);
        let next_cursor = if has_more {
            candidates.last().map(|last| {
                cursor::encode(&FeedCursor {
                    ranked_at: ranked_at.timestamp(),
                    score_bits: score(last, &viewer.declared_categories, ranked_at).to_bits(),
                    last_id: last.id,
                })
            })
        } else {
            None
        };

        Ok(FeedPage {
            items: candidates,
            next_cursor,
        })
    }
}

/// Truncate to whole seconds — the resolution the cursor can carry.
fn whole_seconds(t: DateTime<Utc>) -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp(t.timestamp(), 0).expect("valid unix timestamp")
}

/// Cold-start relevance: recency-decayed popularity, plus a category-match boost.
/// A brand-new post (popularity 0) still scores on recency alone.
fn score(post: &Post, viewer_categories: &[String], now: DateTime<Utc>) -> f64 {
    let age_hours = (now - post.created_at).num_seconds().max(0) as f64 / 3600.0;
    let recency = (-RECENCY_DECAY_PER_HOUR * age_hours).exp();

    let category_bonus = match &post.category {
        Some(c) if viewer_categories.iter().any(|d| d == c) => CATEGORY_BONUS,
        _ => 0.0,
    };

    (1.0 + post.popularity_score) * recency + category_bonus
}
