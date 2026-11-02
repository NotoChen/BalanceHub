<script setup lang="ts">
import { computed } from "vue";
import type {
  AppSettings,
  ProviderInput,
  ProviderNotificationMode,
} from "../../stores/providers";
import { providerProxyModeOptions, proxyModeOptions } from "../../utils/proxy-options";
import type { SelectOption } from "../../utils/liveness-options";

const props = defineProps<{
  draft: ProviderInput;
  settings: AppSettings;
}>();

const notificationModeOptions: SelectOption<ProviderNotificationMode>[] = [
  { label: "跟随全局", value: "inherit" },
  { label: "自定义渠道", value: "custom" },
  { label: "关闭通知", value: "disabled" },
];

const notificationChannelOptions = computed(() =>
  props.settings.notificationChannels.map((channel) => ({
    label: `${channel.name || channel.id}${channel.enabled ? "" : "（已停用）"}`,
    value: channel.id,
  })),
);

const globalProxyLabel = computed(
  () =>
    proxyModeOptions.find((option) => option.value === props.settings.proxyMode)?.label ||
    "未设置",
);

const globalNotificationSummary = computed(() => {
  if (!props.settings.notificationEnabled) return "全局通知已关闭";
  const names = props.settings.notificationChannels
    .filter((channel) => channel.enabled)
    .map((channel) => channel.name || channel.id);
  return names.length ? `全局已启用渠道：${names.join("、")}` : "全局通知已开启，尚未启用渠道";
});

const notificationModeModel = computed({
  get: () => props.draft.notification.mode,
  set: (mode: ProviderNotificationMode) => {
    props.draft.notification.mode = mode;
    if (mode === "custom" && props.draft.notification.channelIds.length === 0) {
      props.draft.notification.channelIds = props.settings.notificationChannels
        .filter((channel) => channel.enabled)
        .map((channel) => channel.id);
    }
  },
});
</script>

<template>
  <section class="provider-form-section">
    <h3 class="provider-form-section-title">代理</h3>
    <div class="provider-form-section-body">
      <a-form-item class="provider-field" label="网络代理">
        <a-select v-model="draft.proxy.mode" :options="providerProxyModeOptions" />
        <template v-if="draft.proxy.mode === 'inherit'" #extra>全局设置：{{ globalProxyLabel }}</template>
      </a-form-item>
      <a-form-item v-if="draft.proxy.mode === 'custom'" class="provider-field" label="代理地址">
        <a-input v-model="draft.proxy.url" placeholder="http://127.0.0.1:7890" allow-clear />
      </a-form-item>
    </div>
  </section>
  <section class="provider-form-section">
    <h3 class="provider-form-section-title">通知</h3>
    <div class="provider-form-section-body">
      <a-form-item class="provider-field" label="通知策略">
        <a-select v-model="notificationModeModel" :options="notificationModeOptions" />
        <template v-if="draft.notification.mode === 'inherit'" #extra>{{ globalNotificationSummary }}</template>
        <template v-else-if="draft.notification.mode === 'custom' && !settings.notificationEnabled" #extra>全局通知已关闭</template>
      </a-form-item>
      <a-form-item v-if="draft.notification.mode === 'custom'" class="provider-field" label="通知渠道">
        <a-select
          v-model="draft.notification.channelIds"
          :options="notificationChannelOptions"
          multiple
          allow-clear
          :disabled="notificationChannelOptions.length === 0"
          placeholder="选择该中转站使用的通知渠道"
        />
        <template v-if="!notificationChannelOptions.length || !draft.notification.channelIds.length" #extra>
          <span v-if="!notificationChannelOptions.length">尚未添加渠道，请先在「设置 → 通知」中添加。</span>
          <span v-else>尚未选择通知渠道。</span>
        </template>
      </a-form-item>
    </div>
  </section>
</template>
