import { defineConfig } from "vite";

// Vite is invoked by Tauri's beforeBuildCommand / beforeDevCommand. We must
// keep the dev server on a fixed port so tauri.conf.json's devUrl matches,
// and disable HMR over the network so the embedded webview can talk to it.
export default defineConfig({
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    host: "127.0.0.1",
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: "es2022",
    minify: "esbuild",
    sourcemap: false,
    outDir: "dist",
  },
});
