<script setup lang="ts">
import { reactive } from "vue";
import SettingsAppearanceSection from "./settings/SettingsAppearanceSection.vue";
import SettingsAutomationSection from "./settings/SettingsAutomationSection.vue";
import SettingsCodexSection from "./settings/SettingsCodexSection.vue";
import SettingsNotificationSection from "./settings/SettingsNotificationSection.vue";
import SettingsSystemSection from "./settings/SettingsSystemSection.vue";
import type { AppSettings } from "../stores/providers";
import type { DurationUnit } from "../utils/duration";

interface ModelProviderIndexItem {
  model: string;
  providers: { id: string; name: string }[];
}

const emit = defineEmits<{
  "update:visible": [visible: boolean];
  "update:globalRefreshAmount": [value: number | undefined];
  "update:globalRefreshUnit": [unit: DurationUnit];
  "probe-codex-cli": [];
  save: [];
  "test-notification": [];
  "export-app-data": [];
  "import-app-data": [];
  "check-for-update": [];
}>();

defineProps<{
  visible: boolean;
  settings: AppSettings;
  livenessModelOptions: string[];
  modelProviderIndex: ModelProviderIndexItem[];
  globalRefreshAmount: number;
  globalRefreshUnit: DurationUnit;
  exportingAppData: boolean;
  importingAppData: boolean;
  appVersion: string;
  checkingForUpdate: boolean;
}>();

const expandedSections = reactive<Record<string, boolean>>({
  appearance: false,
  automation: false,
  codex: false,
  notification: false,
  system: false,
});

function toggleSection(section: string) {
  expandedSections[section] = !expandedSections[section];
}
</script>

<template>
  <a-drawer
    :visible="visible"
    :width="500"
    title="应用设置"
    unmount-on-close
    @update:visible="emit('update:visible', $event)"
  >
    <div class="settings-body">
      <a-form :model="settings" layout="vertical">
        <SettingsAppearanceSection
          :settings="settings"
          :expanded="expandedSections.appearance"
          @toggle="toggleSection('appearance')"
        />

        <SettingsAutomationSection
          :settings="settings"
          :expanded="expandedSections.automation"
          :global-refresh-amount="globalRefreshAmount"
          :global-refresh-unit="globalRefreshUnit"
          @toggle="toggleSection('automation')"
          @update:global-refresh-amount="emit('update:globalRefreshAmount', $event)"
          @update:global-refresh-unit="emit('update:globalRefreshUnit', $event)"
        />

        <SettingsCodexSection
          :settings="settings"
          :expanded="expandedSections.codex"
          :liveness-model-options="livenessModelOptions"
          :model-provider-index="modelProviderIndex"
          @toggle="toggleSection('codex')"
          @probe-codex-cli="emit('probe-codex-cli')"
        />

        <SettingsNotificationSection
          :settings="settings"
          :expanded="expandedSections.notification"
          @toggle="toggleSection('notification')"
          @test-notification="emit('test-notification')"
        />

        <SettingsSystemSection
          :settings="settings"
          :expanded="expandedSections.system"
          :exporting-app-data="exportingAppData"
          :importing-app-data="importingAppData"
          :app-version="appVersion"
          :checking-for-update="checkingForUpdate"
          @toggle="toggleSection('system')"
          @export-app-data="emit('export-app-data')"
          @import-app-data="emit('import-app-data')"
          @check-for-update="emit('check-for-update')"
        />
      </a-form>
    </div>
    <template #footer>
      <div class="drawer-footer">
        <a-button @click="emit('update:visible', false)">取消</a-button>
        <a-button type="primary" @click="emit('save')">保存</a-button>
      </div>
    </template>
  </a-drawer>
</template>
