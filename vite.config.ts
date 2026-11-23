import { defineConfig } from "vite";
import solid from "vite-plugin-solid";
import { soulwindow } from "soulwindow/vite";
import { SOULWINDOW_OPTIONS } from "./src/soulwindow-options";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  clearScreen: false,
  plugins: [solid(), soulwindow(SOULWINDOW_OPTIONS)],
  server: {
    host: host || "localhost",
    port: 1424,
    strictPort: true,
    hmr: host ? { protocol: "ws", host, port: 1424 } : undefined,
    watch: {
      ignored: ["**/src-tauri/**", "**/crates/**", "**/target/**", "**/.rust/**", "**/.scratch/**"],
    },
  },
  build: {
    target: "es2020",
    outDir: "dist",
  },
});
