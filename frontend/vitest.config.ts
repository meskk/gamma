import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

const root = dirname(fileURLToPath(import.meta.url));

// Mirrors the tsconfig path aliases so tests import the same way the app does.
export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@contract": resolve(root, "../backend/bindings"),
      "@": resolve(root, "."),
    },
  },
  test: {
    environment: "jsdom",
    globals: true,
    include: ["{lib,app,components}/**/*.test.{ts,tsx}"],
  },
});
