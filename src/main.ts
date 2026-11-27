import { createApp } from "vue";
import { createPinia } from "pinia";
import { Modal } from "@arco-design/web-vue";
import App from "./App.vue";
import { registerArcoComponents } from "./arco";
import { invoke } from "./api/transport.ts";
import "./arco-styles";
import "./styles/app.css";

/** public/boot-guard.js 在模块脚本之前安装，负责把启动失败从白屏变成可读提示。 */
interface BootGuard {
  mounted: () => void;
  fail: (title: string, detail: string) => void;
}

function bootGuard(): BootGuard | undefined {
  return (window as unknown as { __balancehubBoot?: BootGuard }).__balancehubBoot;
}

/** 同一类错误只上报一次，且总条数封顶，避免渲染循环把日志刷爆。 */
const reportedErrors = new Set<string>();
const MAX_ERROR_REPORTS = 20;

function reportFrontendError(kind: string, error: unknown) {
  const detail = error instanceof Error ? error.stack || error.message : String(error);
  console.error(`[${kind}]`, error);
  const key = `${kind}:${detail.slice(0, 200)}`;
  if (reportedErrors.has(key) || reportedErrors.size >= MAX_ERROR_REPORTS) return;
  reportedErrors.add(key);
  try {
    void invoke("log_frontend_error", {
      message: `${kind}：${detail}`.slice(0, 4000),
    }).catch(() => {});
  } catch {
    // 上报通道自身不可用时不能再抛出，否则会在全局错误回调里形成循环。
  }
}

/** 挂载后不再是白屏问题，但安装版看不到控制台，异步与事件回调异常也要留痕。 */
function watchGlobalErrors() {
  window.addEventListener("unhandledrejection", (event) => {
    reportFrontendError("未处理的异步错误", event.reason);
  });
  window.addEventListener("error", (event) => {
    // 资源加载错误没有 error 对象；ResizeObserver 的循环提示是浏览器噪声，
    // 两者都不作为脚本异常上报。
    if (!event.error || event.message?.includes("ResizeObserver loop")) return;
    reportFrontendError("未捕获的界面错误", event.error);
  });
}

try {
  const app = createApp(App);
  // 挂载后的渲染/生命周期异常不会白屏，但控制台在安装版里看不到；
  // 统一写进宿主日志，保留排查线索。
  app.config.errorHandler = (error, _instance, info) => {
    reportFrontendError(`界面错误（${info}）`, error);
  };
  watchGlobalErrors();
  app.use(createPinia());
  registerArcoComponents(app);
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
