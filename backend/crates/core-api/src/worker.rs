//! Transcode worker loop body. `process_one` handles a single queued job; the
//! `transcode-worker` binary calls it in a loop. Failures are logged and the
//! asset marked failed, so one bad upload never stalls the queue.

use crate::media::MediaService;
use crate::queue::TranscodeQueue;

/// Process one queued job if present. Returns the asset id handled, or `None` if
/// the queue was empty (the caller should then back off briefly).
///
/// Reliable: the job is RESERVED (moved to a processing list) before work starts,
/// and ACKed (removed) only after it reaches a terminal state — so a crash mid-
/// transcode leaves it recoverable rather than lost.
pub async fn process_one(media: &MediaService, queue: &TranscodeQueue) -> Option<i64> {
    let asset_id = match queue.reserve().await {
        Ok(Some(id)) => id,
        Ok(None) => return None,
        Err(err) => {
            tracing::warn!(error = %err, "transcode queue reserve failed");
            return None;
        }
    };

    match media.transcode(asset_id).await {
        Ok(_) => tracing::info!(asset_id, "asset transcoded"),
        Err(err) => {
            tracing::warn!(asset_id, error = ?err, "transcode failed; marking asset failed");
            let _ = media.mark_failed(asset_id).await;
        }
    }
    // Terminal state reached (transcoded or marked failed) → ack. If the ack itself
    // fails, the id stays on the processing list and is re-queued on next startup —
    // at-least-once, never lost.
    if let Err(err) = queue.ack(asset_id).await {
        tracing::warn!(asset_id, error = %err, "failed to ack transcode job; will retry on restart");
    }
    Some(asset_id)
}
