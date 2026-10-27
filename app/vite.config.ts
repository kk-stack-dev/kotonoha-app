import { resolve } from "node:path";
import { defineConfig } from "vite";

// Three pages: first-run setup, the settings window and the recording overlay
export default defineConfig({
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: {
    target: "es2022",
    rollupOptions: {
      input: {
        settings: resolve(import.meta.dirname, "settings.html"),
        setup: resolve(import.meta.dirname, "setup.html"),
        overlay: resolve(import.meta.dirname, "overlay.html"),
      },
    },
  },
});
