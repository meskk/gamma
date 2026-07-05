// The paid-media flow (C2), shared by the reel layer and the post-detail
// view: fetch the asset view (lazily via `enabled` for the reel track),
// unlock, refetch. Presentation stays per-component — this dedupes the flow,
// not the markup.

import { useState } from "react";

import type { MediaAssetView } from "@contract/MediaAssetView";

import { apiFetch } from "@/lib/api";
import { useFetch } from "@/lib/useFetch";

export function useUnlock(
  mediaId: string | null,
  token: string,
  opts: { enabled?: boolean } = {},
) {
  const enabled = opts.enabled !== false && !!mediaId;
  const {
    data: view,
    error: loadError,
    loading,
    reload,
  } = useFetch<MediaAssetView>(() => apiFetch(`/media/${mediaId}`, { token }), [mediaId, token], {
    enabled,
  });
  const [unlocking, setUnlocking] = useState(false);
  const [unlockError, setUnlockError] = useState(false);

  async function unlock() {
    if (!mediaId) return;
    setUnlocking(true);
    setUnlockError(false);
    try {
      await apiFetch<unknown>(`/media/${mediaId}/unlock`, { method: "POST", token });
      reload(); // now entitled → the refetched view carries playback_url
    } catch {
      setUnlockError(true); // the paywall stays visible; the consumer renders its copy
    } finally {
      setUnlocking(false);
    }
  }

  return { view, loadError, unlockError, loading, unlocking, unlock, reload };
}
