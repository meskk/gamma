import { afterEach, describe, expect, it, vi } from "vitest";

import { kindOf, uploadMedia } from "./mediaUpload";

describe("kindOf", () => {
  it("maps MIME prefixes to a media kind", () => {
    expect(kindOf("video/mp4")).toBe("video");
    expect(kindOf("audio/mpeg")).toBe("audio");
    expect(kindOf("image/png")).toBe("image");
  });
});

describe("uploadMedia", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("rejects an unsupported MIME type BEFORE requesting an upload ticket", async () => {
    const fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
    const file = new File(["%PDF-1.7"], "doc.pdf", { type: "application/pdf" });

    await expect(uploadMedia(file, 0, "tok")).rejects.toThrow("unsupported_media_type");
    // The guard runs before any network call, so no asset is ever created/orphaned.
    expect(fetchSpy).not.toHaveBeenCalled();
  });
});
