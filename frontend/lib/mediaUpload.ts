// The presigned media-upload flow: get a ticket → PUT the bytes straight to object
// storage (never through the API) → finalize → (transcode for video). Returns the
// finalized asset view. See the media handler / ADR (media never streams through the
// API server).

import type { MediaAssetView } from "@contract/MediaAssetView";
import type { NewUpload } from "@contract/NewUpload";
import type { UploadTicket } from "@contract/UploadTicket";

import { apiFetch } from "./api";
import type { Wire } from "./wire";

export type MediaKindT = "image" | "video" | "audio";

export function kindOf(mimeType: string): MediaKindT {
  if (mimeType.startsWith("video/")) return "video";
  if (mimeType.startsWith("audio/")) return "audio";
  return "image";
}

export async function uploadMedia(
  file: File,
  unlockPrice: number,
  token: string,
): Promise<MediaAssetView> {
  // Reject unsupported MIME types up front. kindOf() defaults anything unknown
  // to "image", so without this guard a PDF/other type could be uploaded and
  // finalized as a bogus image.
  if (
    !file.type.startsWith("image/") &&
    !file.type.startsWith("video/") &&
    !file.type.startsWith("audio/")
  ) {
    throw new Error("unsupported_media_type");
  }

  const kind = kindOf(file.type);

  // 1. Upload ticket (presigned PUT URL). unlock_price is bigint in the contract but
  //    goes on the wire as a number; Wire<> lets the number through while keeping
  //    the field contract, so a renamed/added required field still breaks the build.
  const newUpload: Wire<NewUpload> = {
    kind,
    content_type: file.type,
    unlock_price: unlockPrice,
  };
  const ticket = await apiFetch<UploadTicket>("/media", {
    method: "POST",
    body: newUpload,
    token,
  });

  // 2. PUT the bytes directly to object storage with the declared content-type.
  const put = await fetch(ticket.upload_url, {
    method: "PUT",
    headers: { "Content-Type": file.type },
    body: file,
  });
  if (!put.ok) {
    throw new Error(`object-store upload failed: ${put.status}`);
  }

  // 3. Finalize: records the size and marks the asset ready.
  const view = await apiFetch<MediaAssetView>(`/media/${ticket.asset_id}/finalize`, {
    method: "POST",
    token,
  });

  // 4. Video: kick off the async HLS transcode (best-effort; the raw playback URL
  //    is usable immediately, the adaptive rendition follows).
  if (kind === "video") {
    await apiFetch<MediaAssetView>(`/media/${ticket.asset_id}/transcode`, {
      method: "POST",
      token,
    }).catch(() => {
      /* transcode is an enhancement; don't fail the upload on it */
    });
  }

  return view;
}
