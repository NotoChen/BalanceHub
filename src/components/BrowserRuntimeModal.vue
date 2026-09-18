<script setup lang="ts">
import { computed, inject, ref, watch } from "vue";
import { Modal } from "@arco-design/web-vue";
import { Globe, Download, RefreshCw } from "@lucide/vue";
import { BROWSER_RUNTIME_CONTEXT } from "../composables/useBrowserRuntime";

const runtime = inject(BROWSER_RUNTIME_CONTEXT);
const state = computed(() => runtime?.state.value ?? null);
const mode = ref("system");
const installing = computed(() => state.value?.phase === "installing");
const busy = computed(() => Boolean(runtime?.pending.value));
const downloadSize = computed(() => Math.ceil(((state.value?.runtimeDownloadBytes ?? 0)
  + (mode.value === "managed" ? state.value?.browserDownloadBytes ?? 0 : 0)) / 1024 / 1024));
watch(() => runtime?.visible.value, (visible) => {
  if (visible) mode.value = state.value?.browser?.managed || !state.value?.systemBrowser ? "managed" : "system";
});
watch(() => state.value?.systemBrowser, (browser) => { if (!browser) mode.value = "managed"; });

function remove() {
  Modal.confirm({ title: "卸载浏览器组件", content: "将删除浏览器组件及临时签到会话。已保存的登录账号、站点凭据和签到记录保留，可在登录账号管理中单独清除。",
    okText: "确认卸载", cancelText: "取消", onOk: () => { void runtime?.uninstall(); } });
}
</script>

<template>
  <a-modal v-if="runtime" v-model:visible="runtime.visible.value" :width="570" :footer="false" modal-class="surface-modal">
    <template #title><span class="browser-component-heading"><Globe :size="20" /> 浏览器登录与验证</span></template>
    <div class="browser-component-content">
      <p>可选组件，用于浏览器登录、导入账号和站点验证码。普通账号密码或 API Key 的接口请求无需安装。首次使用时确认下载。</p>
      <a-alert :type="state?.ready ? 'success' : state?.phase === 'failed' ? 'error' : 'info'">{{ state?.message || '正在检测本机环境…' }}</a-alert>
      <div v-if="state?.browser" class="browser-component-current">
        当前浏览器：<strong>{{ state.browser.name }}</strong>
        <span>{{ state.browser.managed ? 'BalanceHub 独立浏览器' : '本机浏览器程序' }}</span>
        <span v-if="state.browser.version">{{ state.browser.version }}</span>
      </div>
      <template v-if="!installing && state?.canInstall">
        <a-radio-group v-model="mode" direction="vertical" class="browser-component-options">
          <a-radio value="system" :disabled="!state.systemBrowser">
            使用本机浏览器{{ state.systemBrowser ? `：${state.systemBrowser.name}` : '（未检测到）' }}
          </a-radio>
          <a-radio value="managed">安装独立 Chromium，仅供 BalanceHub 使用</a-radio>
        </a-radio-group>
        <p class="browser-component-note">两种方式都为每个登录账号保存独立环境。选择本机浏览器时只下载运行组件；独立模式会额外下载 Chromium。</p>
        <p class="browser-component-note">预计下载不超过 {{ downloadSize }} MiB，使用 App 的网络代理。安装失败或取消会保留原组件。</p>
        <a-button type="primary" :disabled="busy" @click="runtime.install(mode === 'managed')">
          <template #icon><Download :size="15" /></template>
          {{ state.installed ? '确认更新 / 修复组件' : '确认安装组件' }}
        </a-button>
      </template>
      <div v-if="installing" class="browser-component-install">
        <a-progress :percent="state?.progress ?? 0" />
        <p class="browser-component-note">可以关闭此窗口，安装进度会显示在后台任务中。</p>
        <a-button :disabled="busy" @click="runtime.cancel">取消安装</a-button>
      </div>
      <a-alert v-if="runtime.error.value" type="error">{{ runtime.error.value }}</a-alert>
      <div class="browser-component-footer">
        <a-button size="small" :disabled="busy || installing" @click="runtime.refresh(true)"><template #icon><RefreshCw :size="13" /></template>重新检测</a-button>
        <a-button v-if="state?.canUninstall" size="small" status="danger" :disabled="busy" @click="remove">卸载组件</a-button>
      </div>
      <p class="browser-component-note">每次登录或验证前检查环境。组件版本随 App 管理，更新需确认；可在后台任务中查看进度或取消。</p>
      <p class="browser-component-note">Linux 需要图形桌面及 Chromium 系统依赖；Windows ARM64 的独立浏览器使用系统的 x64 兼容支持。</p>
    </div>
  </a-modal>
</template>

<style scoped>
.browser-component-heading { display: inline-flex; align-items: center; gap: 9px; }
.browser-component-content { display: flex; flex-direction: column; align-items: stretch; gap: 16px; color: var(--color-text-1); }
.browser-component-content p { margin: 0; line-height: 1.7; }
.browser-component-current { display: flex; gap: 8px; flex-wrap: wrap; font-size: 13px; }
.browser-component-note { font-size: 12px; color: var(--color-text-3); }
.browser-component-options { padding: 5px 0; }
.browser-component-install { display: grid; gap: 12px; }
.browser-component-footer { display: flex; gap: 8px; }
</style>
