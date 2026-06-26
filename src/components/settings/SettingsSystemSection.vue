<script setup lang="ts">
import { IconDownload, IconRefresh, IconUpload } from "@arco-design/web-vue/es/icon";
import SettingsSection from "./SettingsSection.vue";
import type { AppSettings, ProxyMode } from "../../stores/providers";

interface SelectOption<T extends string = string> {
  label: string;
  value: T;
}

defineProps<{
  settings: AppSettings;
  expanded: boolean;
  exportingAppData: boolean;
  importingAppData: boolean;
  appVersion: string;
  checkingForUpdate: boolean;
}>();

const emit = defineEmits<{
  toggle: [];
  "export-app-data": [];
  "import-app-data": [];
  "check-for-update": [];
}>();

const proxyModeOptions: SelectOption<ProxyMode>[] = [
  { label: "跟随系统代理", value: "system" },
  { label: "不使用代理", value: "noProxy" },
  { label: "自定义代理", value: "custom" },
];
</script>

<template>
  <SettingsSection
    title="网络与系统"
    description="配置代理和开机启动。"
    :expanded="expanded"
    @toggle="emit('toggle')"
  >
    <a-form-item label="代理设置">
      <a-select v-model="settings.proxyMode" :options="proxyModeOptions" />
      <template #extra>默认跟随系统代理，适合使用本机代理软件的场景。</template>
    </a-form-item>
    <a-form-item
      v-if="settings.proxyMode === 'custom'"
      label="自定义代理地址"
      required
    >
      <a-input
        v-model="settings.proxyUrl"
        placeholder="http://127.0.0.1:6152 或 socks5://127.0.0.1:6153"
      />
    </a-form-item>
    <a-form-item label="开机启动">
      <a-switch v-model="settings.launchAtLogin" />
      <template #extra>保存后会写入系统开机启动项。</template>
    </a-form-item>
    <a-form-item label="当前版本">
      <span class="settings-version">{{ appVersion ? `v${appVersion}` : "开发环境" }}</span>
      <template #extra>版本号与 GitHub Release tag 保持一致。</template>
    </a-form-item>
    <a-form-item label="应用更新">
      <a-button :loading="checkingForUpdate" @click="emit('check-for-update')">
        <template #icon><IconRefresh /></template>
        检查更新
      </a-button>
      <template #extra>正式版本会从 GitHub Releases 获取更新说明和当前平台安装包。</template>
    </a-form-item>
    <a-form-item label="配置备份">
      <a-space>
        <a-button :loading="exportingAppData" @click="emit('export-app-data')">
          <template #icon><IconDownload /></template>
          导出配置
        </a-button>
        <a-button :loading="importingAppData" @click="emit('import-app-data')">
          <template #icon><IconUpload /></template>
          导入配置
        </a-button>
      </a-space>
      <template #extra>导入会完整替换当前中转站和应用设置。</template>
    </a-form-item>
  </SettingsSection>
</template>
