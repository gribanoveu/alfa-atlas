import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [react()],
  optimizeDeps: {
    include: ["monaco-editor"],
  },
  build: {
    rollupOptions: {
      // macOS CI runners have ~7 GiB RAM; heavy deps (monaco, shiki, mermaid)
      // need a lower peak. Only there — locally it just makes builds slower.
      ...(process.env.CI ? { maxParallelFileOps: 2 } : {}),
      output: {
        // Mermaid and PlantUML are already split out by their own dynamic
        // `import()`s. Monaco and asciidoctor are not: both are imported
        // statically, so all of Monaco landed in the entry chunk and had to
        // be parsed before the Welcome screen could paint — on a project the
        // user may not even open a file in.
        manualChunks: {
          monaco: ["monaco-editor", "@monaco-editor/react"],
          asciidoctor: ["asciidoctor"],
        },
      },
    },
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
