<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { IconSearch, IconSettings } from "@arco-design/web-vue/es/icon";
import SettingsAppearanceSection from "./settings/SettingsAppearanceSection.vue";
import SettingsAutomationSection from "./settings/SettingsAutomationSection.vue";
import SettingsTerminalSection from "./settings/SettingsTerminalSection.vue";
import SettingsNotificationSection from "./settings/SettingsNotificationSection.vue";
import SettingsSystemSection from "./settings/SettingsSystemSection.vue";
import type { SettingsSaveState } from "../composables/useSettingsController";
import type { AppSettings } from "../stores/providers";
import type { NotificationSendResult } from "../api/app";

const emit = defineEmits<{
  "update:visible": [visible: boolean];
  "retry-save": [];
  "test-notification": [];
  "export-app-data": [];
  "import-app-data": [];
  "check-for-update": [];
}>();

const props = defineProps<{
  visible: boolean;
  settings: AppSettings;
  settingsSaveState: SettingsSaveState;
  settingsSaveError: string;
  testingNotification: boolean;
  notificationTestResult: NotificationSendResult | null;
  notificationTestError: string;
  livenessModelOptions: string[];
  selectedLivenessModelProviders: { id: string; name: string }[];
  exportingAppData: boolean;
  importingAppData: boolean;
  appVersion: string;
  checkingForUpdate: boolean;
}>();

const sections = [
  {
    id: "appearance",
    title: "界面与启动",
    description: "控制应用的外观、启动行为和版本更新。",
    keywords: "界面外观 界面主题 浅色 深色 暗色 亮色 跟随系统 登录后自动启动 自启动 开机 托盘 登录后静默启动 版本更新 检查新版本",
  },
  {
    id: "automation",
    title: "自动化",
    description: "集中管理刷新、签到和自动测活策略。",
    keywords: "额度刷新 自动刷新 刷新间隔 每日签到 自动签到 执行时间 自动测活 执行 Agent CLI 默认模型 支持当前模型 周期策略 执行周期 最短周期 最长周期 固定 随机 超时 提示词策略 固定提示词 提示词模板 话术 数字占位范围 变量素材",
  },
  {
    id: "terminal",
    title: "终端与会话",
    description: "选择终端，管理历史会话索引的存储位置和容量。",
    keywords: "terminal iTerm PowerShell 默认终端 历史会话 会话索引 全文检索 搜索 存储位置 目录 总容量上限 清理 缓存",
  },
  {
    id: "notification",
    title: "通知",
    description: "配置通知渠道，并测试发送结果。",
    keywords: "启用通知 通知渠道 系统通知 Webhook 地址 URL 发送 测试结果 签名密钥",
  },
  {
    id: "system",
    title: "网络与数据",
    description: "管理代理、登录组件和应用备份。",
    keywords: "网络代理 代理策略 代理地址 proxy HTTP HTTPS SOCKS5 自定义 直连 跟随系统 浏览器 Chromium 登录账号 授权 Linux DO GitHub Cookie 凭据 数据备份 导出当前配置 从备份恢复 导入 WebDAV 同步 共享资产 云端 加密 冲突",
  },
] as const;

const searchQuery = ref("");
const searchInput = ref<{ focus(): void } | null>(null);
const settingsScroll = ref<HTMLElement | null>(null);
const searchTerms = computed(() =>
  searchQuery.value.normalize("NFKC").trim().toLocaleLowerCase().split(/\s+/).filter(Boolean),
);
const matchedSections = computed(() => sections.filter((section) => {
  const text = [section.title, section.description, section.keywords].join(" ").toLocaleLowerCase();
  return searchTerms.value.every((term) => text.includes(term));
}));
const matchedSectionIds = computed(() => new Set(matchedSections.value.map((section) => section.id)));
const firstMatchedSection = computed(() => matchedSections.value[0]?.id);

watch(() => props.visible, (visible) => {
  if (visible) searchQuery.value = "";
});
watch(searchQuery, () => {
  if (settingsScroll.value) settingsScroll.value.scrollTop = 0;
}, { flush: "post" });

function focusSearch(event: KeyboardEvent) {
  if (!props.visible || !(event.metaKey || event.ctrlKey) || event.key.toLowerCase() !== "f") return;
  event.preventDefault();
  event.stopPropagation();
  searchInput.value?.focus();
}

function saveStateLabel(state: SettingsSaveState) {
  if (state === "pending") return "等待保存";
  if (state === "saving") return "正在保存";
  if (state === "error") return "修改未保存";
  return "已自动保存";
}
</script>

<template>
  <a-modal
    :visible="visible"
    :width="1020"
    modal-class="surface-modal settings-modal settings-modal-v3"
    :footer="false"
    unmount-on-close
    @keydown="focusSearch"
    @update:visible="emit('update:visible', $event)"
  >
    <template #title>
      <div class="surface-modal-title settings-modal-title">
        <span class="surface-modal-title-icon surface-modal-title-icon-settings"><IconSettings /></span>
        <span class="surface-modal-title-copy">
          <strong>应用设置</strong>
        </span>
        <span class="settings-autosave-status" :class="`is-${settingsSaveState}`" role="status" aria-live="polite">
          <i aria-hidden="true" />{{ saveStateLabel(settingsSaveState) }}
          <a-button v-if="settingsSaveState === 'error'" size="mini" status="danger" @click="emit('retry-save')">重试保存</a-button>
        </span>
      </div>
    </template>

    <div class="settings-workspace settings-workspace-v3">
      <main class="settings-panel" aria-label="应用设置">
        <div class="settings-search-toolbar" role="search" aria-label="查找应用设置">
          <a-input
            ref="searchInput"
            v-model="searchQuery"
            allow-clear
            :input-attrs="{ 'aria-label': '搜索应用设置' }"
            placeholder="搜索设置，如代理、通知、自动签到"
          >
            <template #prefix><IconSearch /></template>
          </a-input>
          <span v-if="searchTerms.length" class="settings-search-count" role="status" aria-live="polite">
            {{ matchedSections.length }} 个相关分区
          </span>
        </div>
        <a-alert v-if="settingsSaveError" class="settings-save-error" type="error" show-icon>
          {{ settingsSaveError }}
        </a-alert>
        <div ref="settingsScroll" class="settings-panel-body">
          <div class="settings-panel-content">
            <a-form :model="settings" :disabled="importingAppData" layout="vertical">
              <section
                v-for="(section, index) in sections"
                v-show="matchedSectionIds.has(section.id)"
                :key="section.id"
                class="settings-flow-section"
                :class="{ 'settings-flow-section-separated': section.id !== firstMatchedSection }"
                :aria-labelledby="'settings-flow-' + section.id"
              >
                <header class="settings-flow-heading">
                  <span aria-hidden="true">{{ String(index + 1).padStart(2, '0') }}</span>
                  <div>
                    <h2 :id="'settings-flow-' + section.id">{{ section.title }}</h2>
                  </div>
                </header>
                <SettingsAppearanceSection
                  v-if="section.id === 'appearance'"
                  :settings="settings"
                  :disabled="importingAppData"
                  :app-version="appVersion"
                  :checking-for-update="checkingForUpdate"
                  @check-for-update="emit('check-for-update')"
                />
                <SettingsAutomationSection
                  v-else-if="section.id === 'automation'"
                  :settings="settings"
                  :liveness-model-options="livenessModelOptions"
                  :selected-liveness-model-providers="selectedLivenessModelProviders"
                />
                <SettingsTerminalSection v-else-if="section.id === 'terminal'" :settings="settings" />
                <SettingsNotificationSection
                  v-else-if="section.id === 'notification'"
                  :settings="settings"
                  :testing="testingNotification"
                  :test-result="notificationTestResult"
                  :test-error="notificationTestError"
                  @test-notification="emit('test-notification')"
                />
                <SettingsSystemSection
                  v-else-if="section.id === 'system'"
                  :settings="settings"
                  :exporting-app-data="exportingAppData"
                  :importing-app-data="importingAppData"
                  @export-app-data="emit('export-app-data')"
                  @import-app-data="emit('import-app-data')"
                />
              </section>
            </a-form>
            <div v-if="!matchedSections.length" class="settings-search-empty" role="status">
              <IconSearch />
              <h2>没有找到相关设置</h2>
              <p>试试“代理”“通知”或“自动签到”等功能名称。</p>
              <a-button @click="searchQuery = ''">显示全部设置</a-button>
            </div>
          </div>
        </div>
      </main>
    </div>
  </a-modal>
</template>
