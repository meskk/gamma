// The core API base URL (includes the /v1 prefix). Configurable per environment;
// defaults to the local backend. Must be a NEXT_PUBLIC_ var to reach the browser.
export const API_BASE_URL =
  process.env.NEXT_PUBLIC_API_BASE_URL ?? "http://localhost:8080/v1";
