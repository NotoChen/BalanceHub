<script setup lang="ts">
import {
  IconDesktop,
  IconLaunch,
  IconMoon,
  IconSun,
} from "@arco-design/web-vue/es/icon";
import type { AppSettings, ThemeMode } from "../../stores/providers";

defineProps<{
  settings: AppSettings;
  expanded?: boolean;
}>();

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

      <div class="settings-theme-options settings-theme-options-v4" role="radiogroup" aria-label="界面主题">
        <button
          v-for="theme in themeOptions"
          :key="theme.value"
          type="button"
          class="settings-theme-option"
          :class="{ active: settings.themeMode === theme.value }"
          role="radio"
          :aria-checked="settings.themeMode === theme.value"
          @click="settings.themeMode = theme.value"
        >
          <span class="settings-theme-swatch" :class="`settings-theme-swatch-${theme.value}`" aria-hidden="true">
            <i /><i /><i />
          </span>
          <span class="settings-theme-option-icon"><component :is="theme.icon" /></span>
          <span>
            <strong>{{ theme.label }}</strong>
          </span>
          <i class="settings-theme-option-check" aria-hidden="true" />
        </button>
      </div>

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
          <a-switch v-model="settings.launchAtLogin" />
        </div>
        <div class="settings-setting-row" :class="{ disabled: !settings.launchAtLogin }">
          <div class="settings-setting-copy">
            <strong>登录后静默启动</strong>
          </div>
          <a-switch
            v-model="settings.launchAtLoginMinimized"
            :disabled="!settings.launchAtLogin"
            title="自启动时不显示主窗口，只保留系统托盘入口"
          />
        </div>
      </div>
    </section>
  </div>
</template>
