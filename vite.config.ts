/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "node:path";

const host = process.env.TAURI_DEV_HOST;

// Vite is used only as the bundler for the Tauri webview. Nothing is served
// publicly; the production build is embedded into the native binary.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: [
      { find: "@", replacement: path.resolve(import.meta.dirname, "src") },
      // monaco-editor >= 0.55 changed its package exports; monaco-yaml still deep-imports
      // `monaco-editor/esm/vs/...`, so map those paths onto the real files.
      {
        find: /^monaco-editor\/esm\/vs\/(.+?)(\.js)?(\?.*)?$/,
        replacement: path.resolve(import.meta.dirname, "node_modules/monaco-editor/esm/vs") + "/$1.js$3",
      },
    ],
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  build: {
    target: process.env.TAURI_ENV_PLATFORM === "windows" ? "chrome105" : "safari15",
    minify: !process.env.TAURI_ENV_DEBUG,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
    chunkSizeWarningLimit: 4096,
  },
  worker: { format: "es" },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.{ts,tsx}"],
  },
});
