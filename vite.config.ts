import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [vue()],

  // 测试用 Vite SSR 加载组件时，Arco 的按组件样式入口是 JS 里导入 CSS；
  // 必须交给 Vite 处理，不能让 Node 按原生 ESM 加载 .css。
  ssr: {
    noExternal: ["@arco-design/web-vue"],
  },

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
          // CodeMirror 只被按需打开的编辑器使用；单独成块，首屏不加载。
          if (/node_modules\/(@codemirror|@lezer|@marijn)\//.test(id)
            || /node_modules\/(style-mod|w3c-keyname|crelt)\//.test(id)) return "editor";
          // Vue 运行时单独成块：缓存粒度只跟框架版本走，不随应用或 Arco 代码变化。
          if (/node_modules\/(@vue\/|vue\/|pinia\/)/.test(id)) return "vue-vendor";
          if (id.includes("node_modules/@tauri-apps")) return "tauri";
          // 其余依赖交给 Rollup 按实际引用分块：只被按需面板引用的库
          // （markdown 渲染、日期/时间选择器等）要留在对应异步块里，
          // 不能因为笼统归入 vendor 或 arco 而被首屏强制加载。
          return undefined;
        },
      },
    },
  },
}));
