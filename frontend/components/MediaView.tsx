"use client";

// Renders a post's attached media. Fetches the asset view (GET /media/:id); if the
// viewer is entitled it shows the player, otherwise it shows the paywall (unlock for
// N gems → POST /media/:id/unlock → refetch → play). Access is gated by the asset's
// own unlock_price, independent of the post.

import { useUnlock } from "@/lib/useUnlock";

export function MediaView({ mediaId, token }: { mediaId: string; token: string }) {
  const { view, loadError, unlockError, unlocking, unlock } = useUnlock(mediaId, token);

  if (loadError) return <p style={{ color: "crimson" }}>Could not load the attached media.</p>;
  if (!view) return <p style={{ color: "#888" }}>Loading media…</p>;

  // Entitled and ready → play. (Video uses the raw URL; adaptive HLS is a follow-up.)
  if (view.playback_url) {
    const url = view.playback_url;
    const style = { maxWidth: "100%", borderRadius: 8 } as const;
    if (view.kind === "image") return <img src={url} alt="" style={style} />;
    if (view.kind === "video") return <video src={url} controls style={style} />;
    if (view.kind === "audio") return <audio src={url} controls />;
    return <a href={url}>Open media</a>;
  }

  // Gated → paywall. An unlock failure keeps the paywall visible (retryable)
  // with its message, instead of replacing the whole view.
  if (Number(view.unlock_price) > 0) {
    return (
      <div style={{ border: "1px dashed #bbb", borderRadius: 8, padding: "1rem", textAlign: "center" }}>
        <p>🔒 This {view.kind} is locked.</p>
        <button type="button" onClick={unlock} disabled={unlocking}>
          {unlocking ? "Unlocking…" : `Unlock for ${String(view.unlock_price)} gems`}
        </button>
        {unlockError && (
          <p style={{ color: "crimson" }}>Unlock failed — do you have enough gems?</p>
        )}
      </div>
    );
  }

  // Free but not finished processing yet.
  return <p style={{ color: "#888" }}>Media is still processing…</p>;
}
