import { resolve } from "node:path";
import babel from "@rolldown/plugin-babel";
import react, { reactCompilerPreset } from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";

const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [
    react(),
    babel({ presets: [reactCompilerPreset()] }),
    tailwindcss(),
  ],
  resolve: {
    alias: {
      "@": resolve(import.meta.dirname, "./src"),
    },
  },

  build: {
    rolldownOptions: {
      output: {
        // Split vendor deps out of the app chunk so the bundle is made of a few
        // cacheable pieces instead of one ~500 kB blob.
        codeSplitting: {
          groups: [
            {
              name: "react-aria",
              test: /[\\/]node_modules[\\/](react-aria-components|@react-aria|@react-stately|@react-types|@internationalized|@swc[\\/]helpers)[\\/]/,
              priority: 30,
            },
            {
              name: "icons",
              test: /[\\/]node_modules[\\/]@untitledui[\\/]icons[\\/]/,
              priority: 30,
            },
            {
              name: "router",
              test: /[\\/]node_modules[\\/]react-router(-dom)?[\\/]/,
              priority: 20,
            },
            {
              name: "react",
              test: /[\\/]node_modules[\\/](react|react-dom|scheduler)[\\/]/,
              priority: 10,
            },
            {
              name: "vendor",
              test: /[\\/]node_modules[\\/]/,
              priority: 0,
            },
          ],
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
