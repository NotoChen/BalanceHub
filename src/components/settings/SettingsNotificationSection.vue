<script setup lang="ts">
import { computed } from "vue";
import type { NotificationSendResult } from "../../api/app";
import {
  IconDelete,
  IconNotification,
  IconPlus,
} from "@arco-design/web-vue/es/icon";
import type {
  AppSettings,
  NotificationChannel,
  NotificationChannelKind,
} from "../../stores/providers";

const props = defineProps<{
  settings: AppSettings;
  testing: boolean;
  testResult: NotificationSendResult | null;
  testError: string;
}>();

const emit = defineEmits<{
  "test-notification": [];
}>();

const enabledChannelCount = computed(() =>
  props.settings.notificationChannels.filter((channel) => channel.enabled).length,
);
const failedTestCount = computed(() =>
  props.testResult?.results.filter((result) => !result.ok).length ?? 0,
);

const channelKindOptions: { label: string; value: NotificationChannelKind }[] = [
  { label: "系统通知", value: "system" },
  { label: "钉钉", value: "dingtalk" },
  { label: "企业微信", value: "wecom" },
  { label: "飞书", value: "feishu" },
  { label: "Slack", value: "slack" },
  { label: "通用 Webhook", value: "generic" },
];

function addChannel() {
  const kind: NotificationChannelKind = "dingtalk";
  props.settings.notificationChannels.push({
    id: `notification-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
    name: availableChannelName(kind),
    kind,
    url: "",
    secret: "",
    enabled: true,
  });
}

function removeChannel(channel: NotificationChannel) {
  props.settings.notificationChannels = props.settings.notificationChannels.filter(
    (item) => item.id !== channel.id,
  );
}

function updateChannelKind(channel: NotificationChannel, kind: NotificationChannelKind) {
  const oldDefaultName = channelKindLabel(channel.kind);
  const currentName = channel.name.trim();
  const suffix = currentName.slice(oldDefaultName.length).trim();
  const shouldUseDefaultName =
    !currentName || currentName === oldDefaultName ||
    (currentName.startsWith(`${oldDefaultName} `) && /^\d+$/.test(suffix));
  channel.kind = kind;
  if (shouldUseDefaultName) {
    channel.name = availableChannelName(kind, channel.id);
  }
  if (kind === "system") {
    channel.url = "";
    channel.secret = "";
  }
}

function channelKindLabel(kind: NotificationChannelKind) {
  return channelKindOptions.find((option) => option.value === kind)?.label || "通知渠道";
}

function availableChannelName(kind: NotificationChannelKind, excludeId?: string) {
  const baseName = channelKindLabel(kind);
  const usedNames = new Set(
    props.settings.notificationChannels
      .filter((channel) => channel.id !== excludeId)
      .map((channel) => channel.name.trim()),
  );
  if (!usedNames.has(baseName)) return baseName;

  let suffix = 2;
  while (usedNames.has(`${baseName} ${suffix}`)) suffix += 1;
  return `${baseName} ${suffix}`;
}

function channelNeedsSecret(kind: NotificationChannelKind) {
  return kind === "dingtalk" || kind === "feishu";
}
</script>

<template>
  <div class="settings-page settings-notification-page">
    <section class="settings-card settings-notification-master">
      <header class="settings-card-header">
        <span class="settings-card-icon"><IconNotification /></span>
        <div>
          <strong>启用通知</strong>
          <span v-if="!settings.notificationEnabled">自动通知已关闭，仍可编辑渠道并发送测试</span>
          <span v-else-if="!enabledChannelCount">请先添加或启用一个通知渠道</span>
        </div>
        <a-button size="small" :loading="testing" :disabled="!enabledChannelCount" @click="emit('test-notification')">
          发送测试
        </a-button>
        <a-switch v-model="settings.notificationEnabled" aria-label="启用通知" />
      </header>
    </section>

    <a-alert v-if="testError" type="error" show-icon>{{ testError }}</a-alert>
    <section v-if="testResult?.results.length" class="settings-card notification-test-results" aria-label="通知测试结果" aria-live="polite">
      <header class="settings-card-header"><div><strong>测试结果</strong></div><span class="settings-card-state" :class="{ active: failedTestCount === 0 }">{{ testResult.sentCount }} 个成功<span v-if="failedTestCount"> · {{ failedTestCount }} 个失败</span></span></header>
      <div v-for="result in testResult.results" :key="result.channelId" class="notification-test-row" :class="{ 'is-error': !result.ok }">
        <strong>{{ result.channelName }}</strong><span>{{ result.ok ? '已发送' : '发送失败' }}</span>
        <p v-if="result.message">{{ result.message }}</p>
      </div>
    </section>

    <section class="settings-card settings-channel-section">
      <header class="settings-card-header">
        <div>
          <strong>通知渠道</strong>
        </div>
        <span class="settings-card-state">已启用 {{ enabledChannelCount }} / {{ settings.notificationChannels.length }}</span>
        <a-button type="outline" size="small" @click="addChannel">
          <template #icon><IconPlus /></template>
          新增渠道
        </a-button>
      </header>

      <p v-if="!settings.notificationChannels.length" class="notification-channel-empty">尚未添加通知渠道，点击「新增渠道」开始配置。</p>
      <div v-else class="notification-channel-list">
        <article
          v-for="channel in settings.notificationChannels"
          :key="channel.id"
          class="notification-channel"
        >
          <div class="notification-channel-header">
            <a-input
              v-model="channel.name"
              class="notification-channel-name"
              size="small"
              placeholder="渠道名称"
              aria-label="通知渠道名称"
            />
            <a-select
              class="notification-channel-kind-select"
              size="small"
              :model-value="channel.kind"
              :options="channelKindOptions"
              aria-label="渠道类型"
              @update:model-value="updateChannelKind(channel, $event as NotificationChannelKind)"
            />
            <div class="notification-channel-actions">
              <span class="notification-channel-state" :class="{ 'is-enabled': channel.enabled }">{{ channel.enabled ? '已启用' : '已停用' }}</span>
              <a-switch v-model="channel.enabled" size="small" :aria-label="`启用${channel.name || '通知渠道'}`" />
              <a-popconfirm
                v-if="channel.id !== 'system'"
                :content="`从应用设置中移除「${channel.name || '未命名渠道'}」？`"
                ok-text="移除渠道"
                cancel-text="取消"
                @ok="removeChannel(channel)"
              >
                <a-button type="text" status="danger" :aria-label="`移除${channel.name || '通知渠道'}`" title="移除渠道">
                  <template #icon><IconDelete /></template>
                </a-button>
              </a-popconfirm>
            </div>
          </div>
          <div
            v-if="channel.kind !== 'system'"
            class="notification-channel-fields"
            :class="{ 'has-secret': channelNeedsSecret(channel.kind) }"
          >
            <label class="notification-channel-field notification-channel-url-field">
              <span>Webhook 地址</span>
              <a-input
                v-model="channel.url"
                placeholder="https://"
                allow-clear
              />
            </label>
            <label v-if="channelNeedsSecret(channel.kind)" class="notification-channel-field">
              <span>签名密钥</span>
              <a-input-password
                v-model="channel.secret"
                placeholder="请输入密钥"
                allow-clear
              />
            </label>
          </div>
        </article>
      </div>
    </section>
  </div>
</template>
