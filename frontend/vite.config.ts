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
    proxy: {
      "/api": "http://localhost:8080",
    },
  },
  test: {
    environment: "happy-dom",
  },
});
