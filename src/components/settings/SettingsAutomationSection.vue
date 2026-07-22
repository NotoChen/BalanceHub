<script setup lang="ts">
import { computed } from "vue";
import { IconCalendarClock, IconRefresh } from "@arco-design/web-vue/es/icon";
import { durationUnitOptions, type DurationUnit } from "../../utils/duration";
import type { AppSettings } from "../../stores/providers";

const props = defineProps<{
  settings: AppSettings;
  expanded?: boolean;
  globalRefreshAmount: number;
  globalRefreshUnit: DurationUnit;
}>();

const emit = defineEmits<{
  toggle: [];
  "update:globalRefreshAmount": [value: number | undefined];
  "update:globalRefreshUnit": [unit: DurationUnit];
}>();

const globalRefreshAmountModel = computed({
  get: () => props.globalRefreshAmount,
  set: (value: number | undefined) => emit("update:globalRefreshAmount", value),
});

const globalRefreshUnitModel = computed({
  get: () => props.globalRefreshUnit,
  set: (value: DurationUnit) => emit("update:globalRefreshUnit", value),
});
</script>

<template>
  <div class="settings-page settings-automation-page">
    <section class="settings-card">
      <header class="settings-card-header">
        <span class="settings-card-icon"><IconRefresh /></span>
        <div>
          <strong>额度刷新</strong>
        </div>
        <span class="settings-card-state" :class="{ active: settings.autoRefreshEnabled }">
          {{ settings.autoRefreshEnabled ? "运行中" : "已关闭" }}
        </span>
      </header>

      <div class="settings-setting-list">
        <div class="settings-setting-row settings-automation-toggle-row">
          <div class="settings-setting-copy">
            <strong>自动刷新</strong>
          </div>
          <a-switch v-model="settings.autoRefreshEnabled" />
        </div>
        <div
          class="settings-setting-row settings-automation-value-row"
          :class="{ disabled: !settings.autoRefreshEnabled }"
        >
          <div class="settings-setting-copy">
            <strong>刷新间隔</strong>
          </div>
          <div class="settings-duration-control">
            <a-input-number
              v-model="globalRefreshAmountModel"
              :min="1"
              :step="1"
              :disabled="!settings.autoRefreshEnabled"
            />
            <a-select
              v-model="globalRefreshUnitModel"
              :options="durationUnitOptions"
              :disabled="!settings.autoRefreshEnabled"
            />
          </div>
        </div>
      </div>
    </section>

    <section class="settings-card">
      <header class="settings-card-header">
        <span class="settings-card-icon settings-card-icon-amber"><IconCalendarClock /></span>
        <div>
          <strong>每日签到</strong>
        </div>
        <span class="settings-card-state" :class="{ active: settings.autoCheckInEnabled }">
          {{ settings.autoCheckInEnabled ? settings.checkInTime : "已关闭" }}
        </span>
      </header>

      <div class="settings-setting-list">
        <div class="settings-setting-row settings-automation-toggle-row">
          <div class="settings-setting-copy">
            <strong>自动签到</strong>
          </div>
          <a-switch v-model="settings.autoCheckInEnabled" />
        </div>
        <div
          class="settings-setting-row settings-automation-value-row"
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
  </div>
</template>
