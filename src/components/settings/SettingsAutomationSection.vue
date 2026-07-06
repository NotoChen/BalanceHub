<script setup lang="ts">
import { computed } from "vue";
import SettingsSection from "./SettingsSection.vue";
import { durationUnitOptions, type DurationUnit } from "../../utils/duration";
import type { AppSettings } from "../../stores/providers";

const props = defineProps<{
  settings: AppSettings;
  expanded: boolean;
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
  <SettingsSection
    title="自动任务"
    description="统一管理刷新和签到的执行节奏。"
    :expanded="expanded"
    @toggle="emit('toggle')"
  >
    <a-form-item label="自动刷新">
      <a-switch v-model="settings.autoRefreshEnabled" />
    </a-form-item>
    <a-form-item label="自动刷新间隔">
      <div class="duration-control">
        <a-input-number v-model="globalRefreshAmountModel" :min="1" :step="1" />
        <a-select v-model="globalRefreshUnitModel" :options="durationUnitOptions" />
      </div>
    </a-form-item>
    <a-form-item label="自动签到">
      <a-switch v-model="settings.autoCheckInEnabled" />
    </a-form-item>
    <a-form-item label="全局签到时间">
      <a-time-picker
        v-model="settings.checkInTime"
        format="HH:mm"
        value-format="HH:mm"
        placeholder="00:00"
        disable-confirm
      />
    </a-form-item>
  </SettingsSection>
</template>
