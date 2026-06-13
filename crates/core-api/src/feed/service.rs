//! Feed business logic: fetch the viewer, pull the candidate set, and rank it
//! with the Phase-1 cold-start ranker (popularity + recency + category match).
//!
//! This deliberately uses no per-user ML — that is the Phase-2 replacement
//! (Dossier §4.2). The ranker is a pure, deterministic function so it is easy to
//! reason about and test.

use chrono::{DateTime, Utc};

use crate::error::ApiError;
use crate::feed::repository::FeedRepository;
use crate::posts::model::Post;
use crate::users::repository::UserRepository;
use db::PgPool;

/// Hard cap on feed size per request.
const MAX_FEED_LIMIT: usize = 100;
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

    /// The viewer's ranked feed, capped at `limit` (clamped to `MAX_FEED_LIMIT`).
    pub async fn personalized(&self, user_id: i64, limit: usize) -> Result<Vec<Post>, ApiError> {
        let viewer = self.users.get(user_id).await?.ok_or(ApiError::NotFound)?;

        let mut candidates = self
            .feed
            .candidates(user_id, &viewer.declared_categories)
            .await?;

        let now = Utc::now();
        candidates.sort_by(|a, b| {
            let sa = score(a, &viewer.declared_categories, now);
            let sb = score(b, &viewer.declared_categories, now);
            // Highest score first; deterministic tie-break by newest id.
            sb.partial_cmp(&sa)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(b.id.cmp(&a.id))
        });

        candidates.truncate(limit.clamp(1, MAX_FEED_LIMIT));
        Ok(candidates)
    }
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
