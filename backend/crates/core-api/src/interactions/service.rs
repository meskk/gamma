//! Interaction capture logic: derive the edge weight from the type and stamp the
//! current epoch, then append. Stamping happens here (not in the client) so the
//! epoch is authoritative and consistent.

use chrono::Utc;
use domain::Epoch;
use econ_params::EconParams;

use crate::comments::repository::CommentRepository;
use crate::error::ApiError;
use crate::interactions::model::{InteractionEvent, InteractionType, NewInteraction};
use crate::interactions::repository::InteractionRepository;
use crate::posts::repository::PostRepository;
use db::PgPool;

#[derive(Clone)]
pub struct InteractionService {
    repo: InteractionRepository,
    /// For the A4f visibility guard: a viewer may only interact with a post they
    /// can see (folds the private-area predicate; cross-repo like FeedService).
    posts: PostRepository,
    /// Resolves a comment target to its post so the SAME A4f guard applies to a
    /// comment-directed interaction (a comment is exactly as visible as its post).
    comments: CommentRepository,
    econ: EconParams,
}

impl InteractionService {
    pub fn new(pool: PgPool) -> Self {
        Self::with_econ(pool, EconParams::default())
    }

    pub fn with_econ(pool: PgPool, econ: EconParams) -> Self {
        Self {
            repo: InteractionRepository::new(pool.clone()),
            posts: PostRepository::new(pool.clone()),
            comments: CommentRepository::new(pool),
            econ,
        }
    }

    pub async fn record(&self, new: NewInteraction) -> Result<InteractionEvent, ApiError> {
        let epoch = Epoch::from_unix_seconds(Utc::now().timestamp());
        let weight = new.r#type.weight(&self.econ.interaction_weights);

        // Normalise the identifiers so the dedup index actually binds to the edge it
        // guards. The edge target is resolved as COALESCE(target_id, post.author,
        // comment.author): when `target_id` is set it WINS and the content ids are
        // irrelevant to the resulting edge — yet they are part of the dedup key
        // (actor, type, epoch, target_id, post_id, comment_id). Left unconstrained,
        // an attacker records N rows {target_id: X, post_id: <N distinct posts>}:
        // each is a distinct dedup tuple (all survive) but all resolve to the SAME
        // edge actor→X, additively inflating X's weight and defeating the 0009/0024
        // index. Fix: collapse to ONE canonical shape — target wins over comment,
        // comment over post (a comment interaction never also carries its post id,
        // else {post, comment} and {comment} would be two tuples for one edge). At
        // least one identifier must be present.
        let (target_id, post_id, comment_id) = normalize_ids(&new)?;

        // A4f: for a CONTENT-targeted event, the actor must be able to SEE the post
        // (ADR 0011 §5) — a private post they aren't entitled to is NotFound, the
        // same as a nonexistent id, so interacting is no existence oracle. A comment
        // is exactly as visible as its post; its guard is ONE query (comment join
        // post join predicate), so "missing" and "unseen" cost identical work —
        // no timing tell either. A direct user→user event (target_id set, content
        // ids cleared above) is unaffected: its weight flows from the target, not
        // any post.
        let visible = match (post_id, comment_id) {
            (Some(p), _) => self.posts.post_visible_to(p, Some(new.actor_id)).await?,
            (None, Some(c)) => {
                self.comments
                    .comment_visible_to(c, Some(new.actor_id))
                    .await?
            }
            (None, None) => true,
        };
        if !visible {
            return Err(ApiError::NotFound);
        }

        // A target user or post that doesn't exist trips a foreign key (migration
        // 0015/0024) — that's a client error (the thing being interacted with is
        // gone), so surface it as 404 rather than a 500. The actor comes from the
        // session, so its FK can't realistically fire.
        self.repo
            .record(
                new.actor_id,
                new.r#type.code(),
                target_id,
                post_id,
                comment_id,
                weight,
                epoch.0 as i32,
            )
            .await
            .map_err(|e| ApiError::on_fk_violation(e, ApiError::NotFound))
    }

    /// Un-like (ADR 0012): VOID every active like the actor holds on this target
    /// (all epochs — the product-level "liked" state is epoch-independent). The
    /// journal row survives with `retracted_at` set; settlement and the read-side
    /// counts skip it. Idempotent: retracting nothing is a success, not a 404 —
    /// a toggle double-fire must not error, and a response independent of the
    /// target's existence opens no oracle. Only `like` is retractable: a comment
    /// event mirrors a comment row that still exists, a follow has its own
    /// DELETE path, and dwell/share never had an "undo" semantic.
    pub async fn retract(&self, new: NewInteraction) -> Result<(), ApiError> {
        if new.r#type != InteractionType::Like {
            return Err(ApiError::Validation("only_like_retractable"));
        }
        let (target_id, post_id, comment_id) = normalize_ids(&new)?;
        self.repo
            .retract(
                new.actor_id,
                new.r#type.code(),
                target_id,
                post_id,
                comment_id,
            )
            .await?;
        Ok(())
    }

    pub async fn list_by_epoch(&self, epoch_k: i32) -> Result<Vec<InteractionEvent>, ApiError> {
        Ok(self.repo.list_by_epoch(epoch_k).await?)
    }
}

/// The canonical (target_id, post_id, comment_id) triple after normalisation.
type CanonicalIds = (Option<i64>, Option<i64>, Option<i64>);

/// The canonical (target, post, comment) shape — see the normalisation comment in
/// `record`. Shared with `retract` so an un-like voids exactly the tuple the
/// corresponding like wrote.
fn normalize_ids(new: &NewInteraction) -> Result<CanonicalIds, ApiError> {
    match (new.target_id, new.post_id, new.comment_id) {
        (Some(t), _, _) => Ok((Some(t), None, None)),
        (None, _, Some(c)) => Ok((None, None, Some(c))),
        (None, Some(p), None) => Ok((None, Some(p), None)),
        (None, None, None) => Err(ApiError::Validation("interaction_requires_target_or_post")),
    }
}
