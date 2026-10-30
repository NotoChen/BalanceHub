<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { IconSearch, IconSettings } from "@arco-design/web-vue/es/icon";
import SettingsAboutSection from "./settings/SettingsAboutSection.vue";
import SettingsAccountsSection from "./settings/SettingsAccountsSection.vue";
import SettingsAppearanceSection from "./settings/SettingsAppearanceSection.vue";
import SettingsAutomationSection from "./settings/SettingsAutomationSection.vue";
import SettingsDataSection from "./settings/SettingsDataSection.vue";
import SettingsNetworkSection from "./settings/SettingsNetworkSection.vue";
import SettingsNotificationSection from "./settings/SettingsNotificationSection.vue";
import SettingsTerminalSection from "./settings/SettingsTerminalSection.vue";
import type { SettingsSaveState } from "../composables/useSettingsController";
import type { AppSettings } from "../stores/providers";
import type { NotificationSendResult } from "../api/app";

const emit = defineEmits<{
  "update:visible": [visible: boolean];
  "retry-save": [];
  "test-notification": [];
  "export-app-data": [];
  "import-app-data": [];
  "install-dev-sample-data": [];
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
  installingDevSample: boolean;
  appVersion: string;
  checkingForUpdate: boolean;
}>();

const sections = [
  {
    id: "appearance",
    title: "界面与启动",
    description: "外观、启动行为、窗口与托盘。",
    keywords: "界面外观 界面主题 浅色 深色 暗色 亮色 跟随系统 登录后自动启动 自启动 开机 托盘 登录后静默启动 关闭按钮 关闭窗口 隐藏到托盘 退出应用 Dock 图标 菜单栏 窗口位置 窗口大小 记住窗口",
  },
  {
    id: "automation",
    title: "自动化",
    description: "刷新、签到、测活与历史记录。",
    keywords: "额度刷新 自动刷新 刷新间隔 失败重试 错开刷新 抖动 每日签到 自动签到 执行时间 自动测活 执行 Agent CLI 默认模型 支持当前模型 周期策略 执行周期 最短周期 最长周期 固定 随机 超时 连续失败 提醒阈值 提示词策略 固定提示词 提示词模板 话术 数字占位范围 变量素材 历史记录 保留 条数",
  },
  {
    id: "notification",
    title: "通知",
    description: "渠道、事件开关、静默时段与去重。",
    keywords: "启用通知 通知渠道 系统通知 Webhook 地址 URL 发送 测试结果 签名密钥 刷新失败 测活失败 签到结果 静默时段 免打扰 夜间 合并 去重 重试",
  },
  {
    id: "terminal",
    title: "终端与会话",
    description: "默认终端与历史会话索引。",
    keywords: "terminal iTerm PowerShell 默认终端 历史会话 会话索引 全文检索 搜索 存储位置 目录 总容量上限 清理 缓存",
  },
  {
    id: "network",
    title: "网络与代理",
    description: "代理策略、直连地址与请求超时。",
    keywords: "网络代理 代理策略 代理地址 proxy HTTP HTTPS SOCKS5 自定义 直连 不走代理 白名单 bypass 跟随系统 请求超时 timeout",
  },
  {
    id: "accounts",
    title: "登录与授权",
    description: "登录账号与浏览器组件。",
    keywords: "浏览器 Chromium 登录账号 授权 Linux DO GitHub Cookie 凭据 浏览器组件",
  },
  {
    id: "data",
    title: "数据与备份",
    description: "WebDAV 同步、导出与恢复。",
    keywords: "数据备份 导出当前配置 从备份恢复 导入 WebDAV 同步 共享资产 云端 加密 冲突 演示数据",
  },
  {
    id: "about",
    title: "关于与更新",
    description: "版本更新、日志与关于信息。",
    keywords: "版本更新 检查新版本 自动检查 检查间隔 日志 日志级别 日志保留 打开日志目录 打开数据目录 目录 GitHub 反馈 Issue 关于",
  },
] as const;

const searchQuery = ref("");
const searchInput = ref<{ focus(): void } | null>(null);
const settingsScroll = ref<HTMLElement | null>(null);
const activeSection = ref<(typeof sections)[number]["id"]>(sections[0].id);
const searchTerms = computed(() =>
  searchQuery.value.normalize("NFKC").trim().toLocaleLowerCase().split(/\s+/).filter(Boolean),
);
const matchedSections = computed(() => sections.filter((section) => {
  const text = [section.title, section.description, section.keywords].join(" ").toLocaleLowerCase();
  return searchTerms.value.every((term) => text.includes(term));
}));
const matchedSectionIds = computed(() => new Set(matchedSections.value.map((section) => section.id)));
/// 搜索时把所有命中的分区堆叠展示，平时只看当前分区——八段连续滚动已经太长。
const visibleSections = computed(() =>
  searchTerms.value.length
    ? matchedSections.value
    : sections.filter((section) => section.id === activeSection.value),
);

function activate(sectionId: (typeof sections)[number]["id"]) {
  activeSection.value = sectionId;
  searchQuery.value = "";
}

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
    :width="1120"
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
      <nav class="settings-nav" aria-label="设置分区">
        <button
          v-for="(section, index) in sections"
          :key="section.id"
          type="button"
          class="settings-nav-item"
          :class="{
            'is-active': !searchTerms.length && section.id === activeSection,
            'is-hit': searchTerms.length > 0 && matchedSectionIds.has(section.id),
          }"
          :aria-current="!searchTerms.length && section.id === activeSection ? 'page' : undefined"
          @click="activate(section.id)"
        >
          <span class="settings-nav-index" aria-hidden="true">{{ String(index + 1).padStart(2, "0") }}</span>
          <span class="settings-nav-copy">
            <strong>{{ section.title }}</strong>
            <small>{{ section.description }}</small>
          </span>
        </button>
      </nav>

      <main class="settings-panel" aria-label="应用设置">
        <div class="settings-search-toolbar" role="search" aria-label="查找应用设置">
          <a-input
            ref="searchInput"
            v-model="searchQuery"
            allow-clear
            :input-attrs="{ 'aria-label': '搜索应用设置' }"
            placeholder="搜索设置，如代理、静默时段、日志"
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
                v-for="section in visibleSections"
                :key="section.id"
                class="settings-section"
                :aria-labelledby="'settings-section-' + section.id"
              >
                <header class="settings-section-heading">
                  <h2 :id="'settings-section-' + section.id">{{ section.title }}</h2>
                  <p>{{ section.description }}</p>
                </header>
                <SettingsAppearanceSection
                  v-if="section.id === 'appearance'"
                  :settings="settings"
                  :disabled="importingAppData"
                />
                <SettingsAutomationSection
                  v-else-if="section.id === 'automation'"
                  :settings="settings"
                  :liveness-model-options="livenessModelOptions"
                  :selected-liveness-model-providers="selectedLivenessModelProviders"
                />
                <SettingsNotificationSection
                  v-else-if="section.id === 'notification'"
                  :settings="settings"
                  :testing="testingNotification"
                  :test-result="notificationTestResult"
                  :test-error="notificationTestError"
                  @test-notification="emit('test-notification')"
                />
                <SettingsTerminalSection v-else-if="section.id === 'terminal'" :settings="settings" />
                <SettingsNetworkSection v-else-if="section.id === 'network'" :settings="settings" />
                <SettingsAccountsSection v-else-if="section.id === 'accounts'" />
                <SettingsDataSection
                  v-else-if="section.id === 'data'"
                  :exporting-app-data="exportingAppData"
                  :importing-app-data="importingAppData"
                  :installing-dev-sample="installingDevSample"
                  @export-app-data="emit('export-app-data')"
                  @import-app-data="emit('import-app-data')"
                  @install-dev-sample-data="emit('install-dev-sample-data')"
                />
                <SettingsAboutSection
                  v-else-if="section.id === 'about'"
                  :settings="settings"
                  :app-version="appVersion"
                  :checking-for-update="checkingForUpdate"
                  @check-for-update="emit('check-for-update')"
                />
              </section>
            </a-form>
            <div v-if="!matchedSections.length" class="settings-search-empty" role="status">
              <IconSearch />
              <h2>没有找到相关设置</h2>
              <p>试试“代理”“静默时段”或“日志”等功能名称。</p>
              <a-button @click="searchQuery = ''">显示全部设置</a-button>
            </div>
          </div>
        </div>
      </main>
    </div>
  </a-modal>
</template>
