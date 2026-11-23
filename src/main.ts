import { createApp } from "vue";
import { createPinia } from "pinia";
import ArcoVue, { Modal } from "@arco-design/web-vue";
import App from "./App.vue";
import "@arco-design/web-vue/dist/arco.css";
import "./styles/app.css";

/** public/boot-guard.js 在模块脚本之前安装，负责把启动失败从白屏变成可读提示。 */
interface BootGuard {
  mounted: () => void;
  fail: (title: string, detail: string) => void;
}

function bootGuard(): BootGuard | undefined {
  return (window as unknown as { __balancehubBoot?: BootGuard }).__balancehubBoot;
}

try {
  const app = createApp(App);
  app.use(createPinia()).use(ArcoVue);
  // 静态弹窗需要继承应用的全局组件注册和注入依赖。
  Modal._context = app._context;
  app.mount("#app");
  bootGuard()?.mounted();
} catch (error) {
  // 挂载阶段的异常不能让窗口停在纯白状态：交给启动守卫展示错误与重试入口。
  const detail =
    error instanceof Error ? error.stack || error.message : String(error);
  bootGuard()?.fail("界面启动失败", detail);
  throw error;
}
