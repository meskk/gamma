/** @type {import('next').NextConfig} */
const nextConfig = {
  // Phase 1a serves media via the API/object-store + CDN, not next/image, so we
  // skip the image optimizer (and its native `sharp` dependency) entirely.
  images: { unoptimized: true },
};

export default nextConfig;
