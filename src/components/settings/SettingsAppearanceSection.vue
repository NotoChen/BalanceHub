<script setup lang="ts">
import { computed, onMounted } from "vue";
import {
  IconDesktop,
  IconLaunch,
  IconMoon,
  IconSun,
} from "@arco-design/web-vue/es/icon";
import type {
  AppSettings,
  ThemeMode,
  TrayClickAction,
  WindowCloseAction,
} from "../../stores/providers";
import { useHostPlatform } from "../../composables/useHostPlatform";
import RadioChoiceGroup from "../RadioChoiceGroup.vue";

defineProps<{
  settings: AppSettings;
  disabled?: boolean;
}>();

const host = useHostPlatform();
onMounted(() => void host.ensure());
/// Dock 图标是 macOS 独有的概念，其他平台不显示这一行。
const showsDockOption = computed(() => host.platform.value === "macos");

const themeOptions: {
  value: ThemeMode;
  label: string;
  icon: typeof IconDesktop;
}[] = [
  { value: "system", label: "跟随系统", icon: IconDesktop },
  { value: "light", label: "浅色", icon: IconSun },
  { value: "dark", label: "深色", icon: IconMoon },
];

const closeActionOptions: { label: string; value: WindowCloseAction }[] = [
  { label: "隐藏到托盘", value: "hideToTray" },
  { label: "退出应用", value: "quit" },
];

const trayClickOptions: { label: string; value: TrayClickAction }[] = [
  { label: "显示窗口", value: "showWindow" },
  { label: "显示 / 收起切换", value: "toggleWindow" },
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
        <span class="settings-card-icon"><IconDesktop /></span>
        <div>
          <strong>窗口与托盘</strong>
          <span>后台刷新、签到和测活依赖进程存活。</span>
        </div>
      </header>

      <div class="settings-setting-list">
        <div class="settings-setting-row settings-setting-row-field">
          <div class="settings-setting-copy">
            <strong>关闭按钮</strong>
            <span>选择「退出应用」后，关闭窗口即结束后台任务。</span>
          </div>
          <a-radio-group v-model="settings.windowCloseAction" type="button" size="small">
            <a-radio v-for="option in closeActionOptions" :key="option.value" :value="option.value">
              {{ option.label }}
            </a-radio>
          </a-radio-group>
        </div>
        <div class="settings-setting-row settings-setting-row-field">
          <div class="settings-setting-copy">
            <strong>点击托盘图标</strong>
            <span>「显示 / 收起切换」适合把窗口当作随手开关的用户。</span>
          </div>
          <a-radio-group v-model="settings.windowTrayClickAction" type="button" size="small">
            <a-radio v-for="option in trayClickOptions" :key="option.value" :value="option.value">
              {{ option.label }}
            </a-radio>
          </a-radio-group>
        </div>
        <div v-if="showsDockOption" class="settings-setting-row">
          <div class="settings-setting-copy">
            <strong>隐藏时移除 Dock 图标</strong>
            <span>窗口隐藏后只保留菜单栏图标，不在 Dock 留驻。</span>
          </div>
          <a-switch v-model="settings.windowHideDockIcon" aria-label="隐藏时移除 Dock 图标" />
        </div>
        <div class="settings-setting-row">
          <div class="settings-setting-copy">
            <strong>记住窗口位置与大小</strong>
            <span>关闭后记住上次的位置；关掉则每次用默认尺寸打开。</span>
          </div>
          <a-switch v-model="settings.windowRestoreState" aria-label="记住窗口位置与大小" />
        </div>
      </div>
    </section>
  </div>
</template>
