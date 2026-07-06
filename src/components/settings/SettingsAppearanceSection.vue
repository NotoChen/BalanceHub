<script setup lang="ts">
import SettingsSection from "./SettingsSection.vue";
import type { AppSettings, ThemeMode } from "../../stores/providers";

interface SelectOption<T extends string = string> {
  label: string;
  value: T;
  disabled?: boolean;
}

defineProps<{
  settings: AppSettings;
  expanded: boolean;
}>();

const emit = defineEmits<{
  toggle: [];
}>();

const themeModeOptions: SelectOption<ThemeMode>[] = [
  { label: "跟随系统", value: "system" },
  { label: "浅色", value: "light" },
  { label: "深色", value: "dark" },
];
</script>

<template>
  <SettingsSection
    title="外观"
    description="控制界面主题。"
    :expanded="expanded"
    @toggle="emit('toggle')"
  >
    <a-form-item label="主题">
      <a-select v-model="settings.themeMode" :options="themeModeOptions" />
    </a-form-item>
  </SettingsSection>
</template>
