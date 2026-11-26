import { cp } from "node:fs/promises";
import { defineConfig } from "vitest/config";

const SYMBOLS_DIR = "node_modules/@material-symbols/svg-400/outlined";

export default defineConfig({
  plugins: [
    {
      // In development icons are served straight from node_modules. For
      // production builds copy them into dist so the app works offline.
      name: "copy-material-symbols",
      apply: "build",
      async closeBundle() {
        await cp(SYMBOLS_DIR, "dist/material-symbols", { recursive: true });
      },
    },
  ],
  server: {
    // The dashboard's public URL in development too, so OAuth redirects
    // work the same. The API server (`mise run dev:server`) is on 8081.
    port: 8080,
    strictPort: true,
    proxy: {
      "/api": "http://127.0.0.1:8081",
    },
  },
  test: {
    environment: "happy-dom",
  },
});
