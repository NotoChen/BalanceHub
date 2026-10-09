<script setup lang="ts">
import { computed, inject, onUnmounted, ref, watch } from "vue";
import { Modal } from "@arco-design/web-vue";
import { open } from "@tauri-apps/plugin-dialog";
import { Globe, Download, FolderOpen, RefreshCw, Play } from "@lucide/vue";
import { BROWSER_RUNTIME_CONTEXT } from "../composables/useBrowserRuntime";
import { inspectBrowserRuntimeBrowser, type BrowserInfo, type BrowserSelection } from "../api/browser-runtime";
import { formatProgress } from "../utils/progress-display";
import ContextDetails from "./ContextDetails.vue";

const runtime = inject(BROWSER_RUNTIME_CONTEXT);
const state = computed(() => runtime?.state.value ?? null);
const selected = ref<BrowserSelection>({ mode: "managed" });
const customBrowser = ref<BrowserInfo | null>(null);
const touched = ref(false);
const picking = ref(false);
const pickerError = ref("");
let pickerRequest = 0;
const operating = computed(() => state.value?.phase === "installing" || state.value?.phase === "checking");
const busy = computed(() => Boolean(runtime?.pending.value));
const browsers = computed(() => {
  const items = state.value?.systemBrowsers ?? [];
  const custom = customBrowser.value;
  return custom && !items.some((item) => item.path === custom.path) ? [...items, custom] : items;
});
const choice = computed({
  get: () => selected.value.mode === "managed" ? "managed" : selected.value.path,
  set: (value: string) => { touched.value = true; selected.value = value === "managed" ? { mode: "managed" } : { mode: "system", path: value }; },
});
const selectedPath = computed(() => selected.value.mode === "system" ? selected.value.path : "");
const downloadBytes = computed(() => (state.value?.runtimeDownloadBytes ?? 0)
  + (selected.value.mode === "managed" ? state.value?.browserDownloadBytes ?? 0 : 0));
const needsInstall = computed(() => !state.value?.runtimeReady || downloadBytes.value > 0);
const downloadSummary = computed(() => {
  const parts = [];
  if (state.value?.runtimeDownloadBytes) parts.push("辅助组件（Node.js 与 Playwright）");
  if (selected.value.mode === "managed" && state.value?.browserDownloadBytes) parts.push("独立 Chromium");
  return "将下载" + parts.join("和") + "，约 " + (downloadBytes.value / 1024 / 1024).toLocaleString("zh-CN", { maximumFractionDigits: 1 }) + " MiB。";
});
const primaryLabel = computed(() => needsInstall.value
  ? state.value?.runtimeReady ? "下载独立浏览器" : "安装辅助组件"
  : touched.value ? "使用并验证" : "验证浏览器");

watch(() => runtime?.visible.value, (visible) => {
  pickerRequest++;
  picking.value = false;
  pickerError.value = "";
  if (visible) {
    touched.value = false;
    customBrowser.value = null;
    selected.value = state.value?.selection ?? { mode: "managed" };
  }
}, { immediate: true });
watch(() => state.value?.selection, (selection) => {
  if (selection && !touched.value) selected.value = selection;
});
watch(() => state.value?.phase, (phase) => {
  if (phase === "ready" && state.value) { selected.value = state.value.selection; touched.value = false; }
});
onUnmounted(() => { pickerRequest++; });

async function browse() {
  const request = ++pickerRequest;
  picking.value = true;
  pickerError.value = "";
  try {
    const path = await open({ title: "选择 Chrome、Edge 或其他 Chromium 浏览器", multiple: false, directory: false });
    if (!path || typeof path !== "string" || request !== pickerRequest || !runtime?.visible.value) return;
    const browser = await inspectBrowserRuntimeBrowser(path);
    if (request !== pickerRequest || !runtime?.visible.value) return;
    customBrowser.value = browser;
    choice.value = browser.path;
  } catch (error) {
    if (request === pickerRequest) pickerError.value = String(error);
  } finally {
    if (request === pickerRequest) picking.value = false;
  }
}

function apply() {
  if (needsInstall.value) void runtime?.install(selected.value);
  else void runtime?.selectBrowser(selected.value);
}
function remove() {
  Modal.confirm({ title: "卸载浏览器辅助组件", content: "将删除辅助组件、独立 Chromium 和临时签到会话。已保存的登录账号、站点凭据和签到记录保留，可在登录账号管理中单独清除。",
    okText: "确认卸载", cancelText: "取消", onOk: () => { void runtime?.uninstall(); } });
}
</script>

<template>
  <a-modal v-if="runtime" v-model:visible="runtime.visible.value" width="min(640px, calc(100vw - 32px))" :footer="false" modal-class="surface-modal">
    <template #title><span class="browser-component-heading"><Globe :size="20" /> 浏览器登录与验证</span></template>
    <div class="browser-component-content">
      <a-alert :type="state?.phase === 'failed' ? 'error' : state?.ready ? 'success' : 'info'" role="status">{{ state?.message || '正在检测本机环境…' }}</a-alert>
      <section v-if="state" class="browser-component-selection" aria-label="浏览器选择">
        <header><strong>选用浏览器</strong><span>辅助组件{{ state.runtimeReady ? '已安装' : state.phase === 'needsUpdate' ? '需要更新' : '待安装' }}</span></header>
        <a-radio-group v-model="choice" direction="vertical" :disabled="busy || operating" class="browser-component-options">
          <a-radio v-for="browser in browsers" :key="browser.path" :value="browser.path">
            <span>{{ browser.name }}</span>
            <span v-if="state.ready && !state.browser?.managed && state.browser?.path === browser.path" class="browser-component-active">使用中 · {{ state.browser.version }}</span>
          </a-radio>
          <a-radio value="managed">
            <span>独立 Chromium</span>
            <span class="browser-component-note">仅供 BalanceHub 使用</span>
          </a-radio>
        </a-radio-group>
        <p v-if="selectedPath" class="browser-component-path" :title="selectedPath">{{ selectedPath }}</p>
        <p v-if="!browsers.length" class="browser-component-note">未在安装位置找到本机浏览器，可手动选择程序。</p>
        <a-button size="small" :disabled="busy || operating" :loading="picking" @click="browse"><template #icon><FolderOpen :size="14" /></template>选择其他浏览器</a-button>
      </section>

      <div v-if="operating" class="browser-component-operation">
        <a-progress v-if="state?.progress !== null && state?.progress !== undefined" :percent="state.progress">
          <template #text="{ percent }">{{ formatProgress(percent) }}</template>
        </a-progress>
        <p class="browser-component-note">可以关闭窗口，进度与取消操作保留在后台任务中。</p>
        <a-button :disabled="busy" @click="runtime.cancel">{{ state?.phase === 'checking' ? '取消验证' : '取消安装' }}</a-button>
      </div>
      <div v-else-if="state?.canInstall" class="browser-component-operation">
        <p v-if="downloadBytes > 0" class="browser-component-note">{{ downloadSummary }}</p>
        <p v-else class="browser-component-note">使用现有辅助组件，以独立临时环境验证浏览器启动。</p>
        <a-button type="primary" :disabled="busy || picking" @click="apply">
          <template #icon><Download v-if="needsInstall" :size="15" /><Play v-else :size="15" /></template>
          {{ primaryLabel }}
        </a-button>
      </div>
      <a-alert v-if="pickerError || runtime.error.value" type="error">{{ pickerError || runtime.error.value }}</a-alert>
      <div class="browser-component-footer">
        <a-button size="small" :disabled="busy || operating" @click="runtime.refresh(true)"><template #icon><RefreshCw :size="13" /></template>重新检测</a-button>
        <a-button v-if="state?.installed" size="small" :disabled="busy || operating || !state.canInstall" @click="runtime.install(selected, true)">修复组件</a-button>
        <a-button v-if="state?.canUninstall" size="small" status="danger" :disabled="busy || operating" @click="remove">卸载组件</a-button>
      </div>
      <ContextDetails label="下载内容与使用说明">
        <p>本机模式复用所选浏览器程序，仅安装 Node.js 和 Playwright 辅助组件；独立模式额外安装 Chromium。下载使用 App 的网络代理。</p>
        <p>各账号使用独立登录环境。普通账号密码或 API Key 的接口请求无需此组件。</p>
        <p>支持 Chromium 系浏览器。Windows 会检查注册表和常见安装路径；macOS 检查应用目录；Linux 检查 PATH 和常见目录。非默认位置可手动选择。</p>
        <p>Linux 需要图形桌面及 Chromium 系统依赖；Windows ARM64 的独立浏览器依赖系统的 x64 兼容支持。</p>
        <p>安装和切换均通过启动验证后生效，失败或取消保留原有选择。验证码要求人工操作时由你完成，App 会显示验证错误原因。</p>
      </ContextDetails>
    </div>
  </a-modal>
</template>

<style scoped>
.browser-component-heading { display: inline-flex; align-items: center; gap: 9px; }
.browser-component-content { display: flex; max-height: calc(100vh - 190px); flex-direction: column; align-items: stretch; gap: 20px; overflow: auto; padding: 2px 4px; color: var(--color-text-1); }
.browser-component-content p { margin: 0; line-height: 1.7; }
.browser-component-selection { display: grid; gap: 12px; padding: 16px; border: 1px solid var(--color-border-2); border-radius: 8px; background: var(--color-fill-1); }
.browser-component-selection header { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 8px; font-size: 13px; }
.browser-component-selection header > span { color: var(--color-text-3); font-size: 12px; }
.browser-component-options { display: flex; gap: 10px; }
.browser-component-options :deep(.arco-radio-label) { display: inline-flex; flex-wrap: wrap; gap: 6px 10px; }
.browser-component-selection > .arco-btn { justify-self: start; }
.browser-component-path { color: var(--color-text-3); font: 11px/1.6 var(--font-mono, monospace); overflow-wrap: anywhere; }
.browser-component-active { color: rgb(var(--success-6)); font-size: 12px; }
.browser-component-note { color: var(--color-text-3); font-size: 12px; }
.browser-component-operation { display: grid; justify-items: start; gap: 12px; }
.browser-component-operation > :deep(.arco-progress) { width: 100%; }
.browser-component-footer { display: flex; flex-wrap: wrap; gap: 8px; }
</style>
