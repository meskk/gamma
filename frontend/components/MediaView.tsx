"use client";

// Renders a post's attached media. Fetches the asset view (GET /media/:id); if the
// viewer is entitled it shows the player, otherwise it shows the paywall (unlock for
// N gems → POST /media/:id/unlock → refetch → play). Access is gated by the asset's
// own unlock_price, independent of the post.

import { useUnlock } from "@/lib/useUnlock";

export function MediaView({ mediaId, token }: { mediaId: string; token: string }) {
  const { view, loadError, unlockError, unlocking, unlock } = useUnlock(mediaId, token);

  if (loadError) return <p style={{ color: "crimson" }}>Medien konnten nicht geladen werden.</p>;
  if (!view) return <p style={{ color: "#888" }}>Medien laden…</p>;

  // Entitled and ready → play. (Video uses the raw URL; adaptive HLS is a follow-up.)
  if (view.playback_url) {
    const url = view.playback_url;
    const style = { maxWidth: "100%", borderRadius: 8 } as const;
    if (view.kind === "image") return <img src={url} alt="" style={style} />;
    if (view.kind === "video") return <video src={url} controls style={style} />;
    if (view.kind === "audio") return <audio src={url} controls />;
    return <a href={url}>Medium öffnen</a>;
  }

  // Gated → paywall. An unlock failure keeps the paywall visible (retryable)
  // with its message, instead of replacing the whole view.
  if (Number(view.unlock_price) > 0) {
    return (
      <div style={{ border: "1px dashed #bbb", borderRadius: 8, padding: "1rem", textAlign: "center" }}>
        <p>🔒 Dieses {view.kind === "video" ? "Video" : "Medium"} ist gesperrt.</p>
        <button type="button" onClick={unlock} disabled={unlocking}>
          {unlocking ? "Wird freigeschaltet…" : `Für ${String(view.unlock_price)} Gems freischalten`}
        </button>
        {unlockError && (
          <p style={{ color: "crimson" }}>Freischalten fehlgeschlagen — genug Gems?</p>
        )}
      </div>
    );
  }

  // Free but not finished processing yet.
  return <p style={{ color: "#888" }}>Medium wird noch verarbeitet…</p>;
}
