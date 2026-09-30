<script setup lang="ts">
import {
  IconDesktop,
  IconLaunch,
  IconMoon,
  IconRefresh,
  IconSun,
} from "@arco-design/web-vue/es/icon";
import type { AppSettings, ThemeMode } from "../../stores/providers";
import { formatAppVersionLabel } from "../../utils/app-version";
import RadioChoiceGroup from "../RadioChoiceGroup.vue";

defineProps<{
  settings: AppSettings;
  appVersion: string;
  checkingForUpdate: boolean;
  disabled?: boolean;
}>();

const emit = defineEmits<{ "check-for-update": [] }>();

const themeOptions: {
  value: ThemeMode;
  label: string;
  icon: typeof IconDesktop;
}[] = [
  { value: "system", label: "跟随系统", icon: IconDesktop },
  { value: "light", label: "浅色", icon: IconSun },
  { value: "dark", label: "深色", icon: IconMoon },
];

</script>

<template>
  <div class="settings-page settings-general-page">
    <section class="settings-card settings-appearance-card">
      <header class="settings-card-header">
        <span class="settings-card-icon"><IconDesktop /></span>
        <div>
          <strong>界面外观</strong>
        </div>
      </header>

      <RadioChoiceGroup
        v-model="settings.themeMode"
        :options="themeOptions"
        :disabled="disabled"
        label="界面主题"
        class="settings-theme-options settings-theme-options-v4"
        option-class="settings-theme-option"
      >
        <template #default="{ option: theme }">
          <span class="settings-theme-swatch" :class="`settings-theme-swatch-${theme.value}`" aria-hidden="true">
            <i /><i /><i />
          </span>
          <span class="settings-theme-option-icon"><component :is="theme.icon" /></span>
          <span class="settings-theme-option-label">
            <strong>{{ theme.label }}</strong>
          </span>
          <i class="settings-theme-option-check" aria-hidden="true" />
        </template>
      </RadioChoiceGroup>

    </section>

    <section class="settings-card">
      <header class="settings-card-header">
        <span class="settings-card-icon settings-card-icon-green"><IconLaunch /></span>
        <div>
          <strong>启动行为</strong>
        </div>
      </header>

      <div class="settings-setting-list">
        <div class="settings-setting-row">
          <div class="settings-setting-copy">
            <strong>登录后自动启动</strong>
          </div>
          <a-switch v-model="settings.launchAtLogin" aria-label="登录后自动启动" />
        </div>
        <div class="settings-setting-row" :class="{ disabled: !settings.launchAtLogin }">
          <div class="settings-setting-copy">
            <strong>登录后静默启动</strong>
            <span>自启动时仅保留托盘入口，不打开主窗口。</span>
          </div>
          <a-switch
            v-model="settings.launchAtLoginMinimized"
            :disabled="!settings.launchAtLogin"
            aria-label="登录后静默启动"
            title="自启动时不显示主窗口，只保留系统托盘入口"
          />
        </div>
      </div>
    </section>

    <section class="settings-card">
      <header class="settings-card-header">
        <span class="settings-card-icon settings-card-icon-green"><IconRefresh /></span>
        <div><strong>版本更新</strong></div>
        <span class="settings-version-badge">{{ formatAppVersionLabel(appVersion) }}</span>
      </header>
      <div class="settings-setting-row">
        <div class="settings-setting-copy"><strong>检查新版本</strong></div>
        <a-button :loading="checkingForUpdate" @click="emit('check-for-update')">
          <template #icon><IconRefresh /></template>检查更新
        </a-button>
      </div>
    </section>
  </div>
</template>
