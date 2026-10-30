import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [vue()],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    // 固定监听 IPv4：macOS 上 localhost 可能优先解析到 ::1，而代理或网络栈
    // 不保证两端都可达，devUrl 与监听地址必须落在同一个协议族。
    host: host || "127.0.0.1",
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
  build: {
    chunkSizeWarningLimit: 1200,
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (id.includes("node_modules/@taplo/")) return "formatter-toml";
          if (id.includes("node_modules/prettier/")) return "formatter-text";
          if (id.includes("node_modules/@arco-design")) return "arco";
          if (id.includes("node_modules/@tauri-apps")) return "tauri";
          if (id.includes("node_modules/vue") || id.includes("node_modules/pinia")) {
            return "vue-vendor";
          }
          if (id.includes("node_modules")) return "vendor";
          return undefined;
        },
      },
    },
  },
}));
