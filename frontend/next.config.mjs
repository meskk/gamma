/** @type {import('next').NextConfig} */

// The bearer token is stored in sessionStorage (JS-readable) — see lib/auth.tsx.
// A Content-Security-Policy is the compensating control: it constrains where
// scripts may come from and where the page may connect, shrinking the XSS surface
// that could exfiltrate that token. This is a reasonable Phase-1a default, not a
// hardened production policy (see the `'unsafe-inline'` notes below).
//
// The API origin must be reachable via connect-src; derive it from the same env var
// the client uses so a deployment override stays in sync.
const apiBase = process.env.NEXT_PUBLIC_API_BASE_URL || "http://localhost:8080/v1";
let apiOrigin = "http://localhost:8080";
try {
  apiOrigin = new URL(apiBase).origin;
} catch {
  // Keep the default if the env var isn't a valid absolute URL.
}

const csp = [
  "default-src 'self'",
  // 'unsafe-inline' is required for Next's inline bootstrap scripts and, in dev,
  // for React refresh ('unsafe-eval'). Tighten with nonces if this hardens later.
  "script-src 'self' 'unsafe-inline'" + (process.env.NODE_ENV !== "production" ? " 'unsafe-eval'" : ""),
  // Inline <style> + style= attributes (the glass login) need 'unsafe-inline'.
  "style-src 'self' 'unsafe-inline'",
  // Fonts are self-hosted via next/font — same origin, no fonts.googleapis.com.
  "font-src 'self'",
  // Media/images can come from the object store / presigned URLs (any https host)
  // plus data/blob URLs used by previews.
  "img-src 'self' data: blob: https:",
  "media-src 'self' blob: https:",
  // XHR/fetch targets: the app itself and the core API.
  `connect-src 'self' ${apiOrigin}`,
  "frame-ancestors 'none'",
  "base-uri 'self'",
  "form-action 'self'",
]
  .join("; ")
  .trim();

const nextConfig = {
  // Hide the Next.js dev-mode indicator (the floating "N" badge in the corner) so
  // it doesn't sit on top of the full-bleed screens (login, feed) during dev.
  devIndicators: false,

  // Phase 1a serves media via the API/object-store + CDN, not next/image, so we
  // skip the image optimizer (and its native `sharp` dependency) entirely.
  images: { unoptimized: true },

  async headers() {
    return [
      {
        source: "/:path*",
        headers: [
          { key: "Content-Security-Policy", value: csp },
          { key: "X-Content-Type-Options", value: "nosniff" },
          { key: "Referrer-Policy", value: "strict-origin-when-cross-origin" },
          { key: "X-Frame-Options", value: "DENY" },
        ],
      },
    ];
  },
};

export default nextConfig;
