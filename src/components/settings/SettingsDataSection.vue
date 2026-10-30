<script setup lang="ts">
import {
  IconDownload,
  IconExperiment,
  IconStorage,
  IconUpload,
} from "@arco-design/web-vue/es/icon";
import SettingsCloudSyncSection from "./SettingsCloudSyncSection.vue";

defineProps<{
  exportingAppData: boolean;
  importingAppData: boolean;
  installingDevSample: boolean;
}>();

const emit = defineEmits<{
  "export-app-data": [];
  "import-app-data": [];
  "install-dev-sample-data": [];
}>();

/// 演示数据只在开发构建里出现；安装版后端也没有对应的 command。
const isDevelopmentBuild = import.meta.env.DEV;
</script>

<template>
  <div class="settings-page settings-data-page">
    <SettingsCloudSyncSection />
    <section class="settings-card">
      <header class="settings-card-header">
        <span class="settings-card-icon settings-card-icon-amber"><IconStorage /></span>
        <div>
          <strong>数据备份</strong>
          <span>备份文件包含中转站、认证凭据和应用设置。</span>
        </div>
      </header>

      <div class="settings-setting-row">
        <div class="settings-setting-copy">
          <strong>导出当前配置</strong>
          <span>备份中转站、认证凭据和应用设置，用于迁移或恢复。</span>
        </div>
        <a-button :loading="exportingAppData" :disabled="importingAppData" @click="emit('export-app-data')">
          <template #icon><IconDownload /></template>
          导出备份
        </a-button>
      </div>
      <div class="settings-setting-row">
        <div class="settings-setting-copy">
          <strong>从备份恢复</strong>
          <span>选择备份文件，确认后替换当前中转站和应用设置。</span>
        </div>
        <a-button :loading="importingAppData" :disabled="exportingAppData" @click="emit('import-app-data')">
          <template #icon><IconUpload /></template>
          选择备份
        </a-button>
      </div>
      <div v-if="isDevelopmentBuild" class="settings-setting-row">
        <div class="settings-setting-copy">
          <strong>追加演示中转站</strong>
          <span>仅开发版：在开发版现有配置上追加编造的站点，覆盖 Key、订阅、OAuth 与网关各类卡片，便于检查界面；真实站点原样保留。演示卡片名为「演示 · …」，重复点击不会重复追加。自动刷新、签到与测活保持关闭。</span>
        </div>
        <a-button
          :loading="installingDevSample"
          :disabled="exportingAppData || importingAppData"
          @click="emit('install-dev-sample-data')"
        >
          <template #icon><IconExperiment /></template>
          追加演示数据
        </a-button>
      </div>
    </section>
  </div>
</template>
