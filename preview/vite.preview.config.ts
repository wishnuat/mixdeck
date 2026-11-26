import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "node:path";

// Serves the real UI in a browser with the Tauri APIs mocked (UI review and README screenshots).
export default defineConfig({
  root: path.resolve(__dirname, ".."),
  plugins: [react()],
  resolve: {
    alias: {
      "@tauri-apps/api/core": path.resolve(__dirname, "mock-core.ts"),
      "@tauri-apps/api/event": path.resolve(__dirname, "mock-event.ts"),
    },
  },
  server: { port: 1440, strictPort: true },
});
