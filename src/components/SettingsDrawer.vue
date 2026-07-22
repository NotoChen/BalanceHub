<script setup lang="ts">
import { computed, ref, watch } from "vue";
import {
  IconBgColors,
  IconCalendarClock,
  IconCommand,
  IconNotification,
  IconSettings,
  IconTool,
} from "@arco-design/web-vue/es/icon";
import SettingsAppearanceSection from "./settings/SettingsAppearanceSection.vue";
import SettingsAutomationSection from "./settings/SettingsAutomationSection.vue";
import SettingsCodexSection from "./settings/SettingsCodexSection.vue";
import SettingsNotificationSection from "./settings/SettingsNotificationSection.vue";
import SettingsSystemSection from "./settings/SettingsSystemSection.vue";
import type { SettingsSaveState } from "../composables/useSettingsController";
import type { AppSettings } from "../stores/providers";
import type { DurationUnit } from "../utils/duration";

interface ModelProviderIndexItem {
  model: string;
  providers: { id: string; name: string }[];
}

type SettingsSectionKey = "appearance" | "automation" | "codex" | "notification" | "system";

const emit = defineEmits<{
  "update:visible": [visible: boolean];
  "update:globalRefreshAmount": [value: number | undefined];
  "update:globalRefreshUnit": [unit: DurationUnit];
  "test-notification": [];
  "export-app-data": [];
  "import-app-data": [];
  "check-for-update": [];
}>();

const props = defineProps<{
  visible: boolean;
  settings: AppSettings;
  settingsSaveState: SettingsSaveState;
  livenessModelOptions: string[];
  modelProviderIndex: ModelProviderIndexItem[];
  globalRefreshAmount: number;
  globalRefreshUnit: DurationUnit;
  exportingAppData: boolean;
  importingAppData: boolean;
  appVersion: string;
  checkingForUpdate: boolean;
}>();

const activeSection = ref<SettingsSectionKey>("appearance");

const sectionMeta: Record<SettingsSectionKey, {
  label: string;
  icon: typeof IconSettings;
}> = {
  appearance: {
    label: "常规",
    icon: IconBgColors,
  },
  automation: {
    label: "自动化",
    icon: IconCalendarClock,
  },
  codex: {
    label: "Agent 与终端",
    icon: IconCommand,
  },
  notification: {
    label: "通知",
    icon: IconNotification,
  },
  system: {
    label: "网络与数据",
    icon: IconTool,
  },
};

const activeMeta = computed(() => sectionMeta[activeSection.value]);

watch(
  () => props.visible,
  (visible) => {
    if (visible) {
      activeSection.value = "appearance";
    }
  },
);

function selectSection(section: SettingsSectionKey) {
  activeSection.value = section;
}

function saveStateLabel(state: SettingsSaveState) {
  if (state === "pending") return "等待保存";
  if (state === "saving") return "正在保存";
  if (state === "error") return "保存失败";
  return "已保存";
}
</script>

<template>
  <a-modal
    :visible="visible"
    :width="1020"
    modal-class="surface-modal settings-modal settings-modal-v3"
    :footer="false"
    unmount-on-close
    @update:visible="emit('update:visible', $event)"
  >
    <template #title>
      <div class="surface-modal-title settings-modal-title">
        <span class="surface-modal-title-icon surface-modal-title-icon-settings"><IconSettings /></span>
        <span class="surface-modal-title-copy">
          <strong>应用设置</strong>
        </span>
      </div>
    </template>

    <div class="settings-workspace settings-workspace-v3">
      <nav class="settings-topbar" aria-label="设置模块">
        <div class="settings-topbar-track">
          <button
            v-for="(meta, key) in sectionMeta"
            :key="key"
            type="button"
            class="settings-topbar-item"
            :class="{ active: activeSection === key }"
            :aria-current="activeSection === key ? 'page' : undefined"
            @click="selectSection(key as SettingsSectionKey)"
          >
            <span class="settings-topbar-icon"><component :is="meta.icon" /></span>
            <strong>{{ meta.label }}</strong>
          </button>
        </div>
      </nav>

      <main class="settings-panel">
        <header class="settings-panel-header">
          <div class="settings-panel-header-inner">
            <div class="settings-panel-heading">
              <h2>{{ activeMeta.label }}</h2>
            </div>
            <span class="settings-autosave-status" :class="`is-${settingsSaveState}`">
              <i />
              {{ saveStateLabel(settingsSaveState) }}
            </span>
          </div>
        </header>

        <div class="settings-panel-body">
          <div class="settings-panel-content">
            <a-form :model="settings" layout="vertical">
              <SettingsAppearanceSection
                v-if="activeSection === 'appearance'"
                :settings="settings"
                :expanded="true"
              />
              <SettingsAutomationSection
                v-else-if="activeSection === 'automation'"
                :settings="settings"
                :expanded="true"
                :global-refresh-amount="globalRefreshAmount"
                :global-refresh-unit="globalRefreshUnit"
                @update:global-refresh-amount="emit('update:globalRefreshAmount', $event)"
                @update:global-refresh-unit="emit('update:globalRefreshUnit', $event)"
              />
              <SettingsCodexSection
                v-else-if="activeSection === 'codex'"
                :settings="settings"
                :expanded="true"
                :liveness-model-options="livenessModelOptions"
                :model-provider-index="modelProviderIndex"
              />
              <SettingsNotificationSection
                v-else-if="activeSection === 'notification'"
                :settings="settings"
                :expanded="true"
                @test-notification="emit('test-notification')"
              />
              <SettingsSystemSection
                v-else
                :settings="settings"
                :expanded="true"
                :exporting-app-data="exportingAppData"
                :importing-app-data="importingAppData"
                :app-version="appVersion"
                :checking-for-update="checkingForUpdate"
                @export-app-data="emit('export-app-data')"
                @import-app-data="emit('import-app-data')"
                @check-for-update="emit('check-for-update')"
              />
            </a-form>
          </div>
        </div>

        <footer class="settings-panel-footer">
          <div class="settings-panel-footer-inner">
            <div class="drawer-footer">
              <a-button type="secondary" @click="emit('update:visible', false)">关闭</a-button>
            </div>
          </div>
        </footer>
      </main>
    </div>
  </a-modal>
</template>
