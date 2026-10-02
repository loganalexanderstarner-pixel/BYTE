import { readFileSync } from "node:fs";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const pkg = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8")) as { version: string };

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
  },
  server: {
    strictPort: true,
    port: 1420,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    // macOS 13's WebKit (Safari 16) is the oldest engine BYTE runs in.
    target: "safari16",
    sourcemap: false,
    chunkSizeWarningLimit: 1500,
  },
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
    // tokens.test.ts reads the theme CSS as text (?raw) to check contrast.
    css: { include: [/tokens\.css/] },
  },
});
