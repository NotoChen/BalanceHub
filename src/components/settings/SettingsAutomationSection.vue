<script setup lang="ts">
import { IconCalendarClock, IconRefresh } from "@arco-design/web-vue/es/icon";
import DurationInput from "../DurationInput.vue";
import SettingsLivenessSection from "./SettingsLivenessSection.vue";
import type { AppSettings } from "../../stores/providers";

defineProps<{
  settings: AppSettings;
  livenessModelOptions: string[];
  selectedLivenessModelProviders: { id: string; name: string }[];
}>();
</script>

<template>
  <div class="settings-page settings-automation-page">
    <section class="settings-card">
      <header class="settings-card-header">
        <span class="settings-card-icon"><IconRefresh /></span>
        <div>
          <strong>额度刷新</strong>
        </div>
        <a-switch v-model="settings.autoRefreshEnabled" aria-label="自动刷新额度" />
      </header>

      <div class="settings-setting-list">
        <div
          class="settings-setting-row settings-setting-row-field"
          :class="{ disabled: !settings.autoRefreshEnabled }"
        >
          <div class="settings-setting-copy">
            <strong>刷新间隔</strong>
            <span>中转站可单独设置间隔，最短 30 秒。</span>
          </div>
          <DurationInput v-model="settings.refreshInterval" :min="30" :disabled="!settings.autoRefreshEnabled" label="刷新间隔" />
        </div>
      </div>
    </section>

    <section class="settings-card">
      <header class="settings-card-header">
        <span class="settings-card-icon settings-card-icon-amber"><IconCalendarClock /></span>
        <div>
          <strong>每日签到</strong>
        </div>
        <a-switch v-model="settings.autoCheckInEnabled" aria-label="每日自动签到" />
      </header>

      <div class="settings-setting-list">
        <div
          class="settings-setting-row settings-setting-row-field"
          :class="{ disabled: !settings.autoCheckInEnabled }"
        >
          <div class="settings-setting-copy">
            <strong>执行时间</strong>
          </div>
          <a-time-picker
            v-model="settings.checkInTime"
            format="HH:mm"
            value-format="HH:mm"
            placeholder="00:00"
            disable-confirm
            :disabled="!settings.autoCheckInEnabled"
          />
        </div>
      </div>
    </section>
    <SettingsLivenessSection
      :settings="settings"
      :liveness-model-options="livenessModelOptions"
      :selected-liveness-model-providers="selectedLivenessModelProviders"
    />
  </div>
</template>
