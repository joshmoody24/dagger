import { defineConfig } from "vite";
import solid from "vite-plugin-solid";

export default defineConfig({
  plugins: [solid()],
  // Tauri looks for the page here while developing, and for the build in dist.
  server: { port: 1420, strictPort: true },
  build: { target: "esnext" },
});
