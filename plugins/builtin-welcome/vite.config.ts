import { viteSingleFile } from "vite-plugin-singlefile";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [viteSingleFile()],
  build: {
    target: "es2022",
    assetsInlineLimit: Number.MAX_SAFE_INTEGER,
    cssCodeSplit: false
  },
  test: {
    environment: "jsdom"
  }
});
