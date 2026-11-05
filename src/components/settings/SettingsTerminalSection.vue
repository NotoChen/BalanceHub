<script setup lang="ts">
import { IconDesktop } from "@arco-design/web-vue/es/icon";
import SettingsTerminalManager from "./SettingsTerminalManager.vue";
import type { AppSettings } from "../../stores/providers";

defineProps<{
  settings: AppSettings;
}>();
</script>

<template>
  <div class="settings-page settings-cli-page">
    <section class="settings-card settings-terminal-card">
      <header class="settings-card-header">
        <span class="settings-card-icon settings-card-icon-amber"><IconDesktop /></span>
        <div><strong>终端</strong></div>
      </header>
      <SettingsTerminalManager :settings="settings" />
    </section>
  </div>
</template>
