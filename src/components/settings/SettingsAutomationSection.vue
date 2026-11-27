<script setup lang="ts">
import { IconCalendarClock, IconClockCircle, IconHistory, IconRefresh } from "@arco-design/web-vue/es/icon";
import { computed } from "vue";
// 定时签到只在设置抽屉里用，和抽屉一起按需加载。
import { TimePicker as ATimePicker } from "@arco-design/web-vue";
import "@arco-design/web-vue/es/time-picker/style/css.js";
import DurationInput from "../DurationInput.vue";
import SettingsLivenessSection from "./SettingsLivenessSection.vue";
import { useHostCapabilities } from "../../composables/useHostCapabilities";
import { useCliRuntimeStore } from "../../stores/cli-runtime";
import { agentCliLabel } from "../../utils/cli-environment";
import { formatDuration } from "../../utils/duration";
import type { AppSettings } from "../../stores/providers";

const props = defineProps<{
  settings: AppSettings;
  livenessModelOptions: string[];
  selectedLivenessModelProviders: { id: string; name: string }[];
}>();

/// 测活要启动本机 Agent CLI；浏览器版没有这条链路，不展示相应设置。
const { commandAvailable } = useHostCapabilities();
const livenessAvailable = computed(() => commandAvailable("probe_cli_tools"));
const store = useCliRuntimeStore();

interface AutomationSummaryItem {
  key: string;
  label: string;
  detail: string;
  on: boolean;
}

/// 自动行为摘要把三个开关背后的真实计划写出来：用户不必逐项回忆间隔、
/// 时间和执行 Agent，也不会把「开着但没到点」误认成没生效。
const automationSummary = computed<AutomationSummaryItem[]>(() => {
  const settings = props.settings;
  const retry = Math.max(0, Number(settings.refreshRetryCount) || 0);
  const items: AutomationSummaryItem[] = [
    {
      key: "refresh",
      label: "额度刷新",
      on: Boolean(settings.autoRefreshEnabled),
      detail: settings.autoRefreshEnabled
        ? `每 ${formatDuration(settings.refreshInterval)} 刷新一轮全部启用站点`
          + (retry ? `，失败按 1、2 分钟递增重试 ${retry} 次` : "，失败不自动重试")
          + (settings.refreshJitter ? "，各站启动时间随机错开" : "")
        : "已关闭，只在手动刷新时读取额度与模型",
    },
    {
      key: "checkIn",
      label: "每日签到",
      on: Boolean(settings.autoCheckInEnabled),
      detail: settings.autoCheckInEnabled
        ? `每天 ${settings.checkInTime || "00:00"} 检查并签到尚未签到的站点`
        : "已关闭，只在手动点击签到卡时执行",
    },
  ];
  if (livenessAvailable.value) {
    const mode = settings.livenessIntervalMode === "fixed"
      ? `每 ${formatDuration(settings.livenessInterval)}`
      : `随机间隔 ${formatDuration(settings.livenessRandomMinInterval)}–${formatDuration(settings.livenessRandomMaxInterval)}`;
    const model = settings.livenessModel.trim();
    items.push({
      key: "liveness",
      label: "自动测活",
      on: Boolean(settings.livenessEnabled),
      detail: settings.livenessEnabled
        ? `${mode} 用 ${agentCliLabel(store.cliEnvironmentProbe, settings.livenessCliKind)} 发起真实请求测活`
          + (model ? ` · 模型 ${model}` : " · 默认模型")
          + "，会消耗少量额度"
        : "已关闭，只在手动测活时启动 Agent",
    });
  }
  return items;
});
</script>

<template>
  <div class="settings-page settings-automation-page">
    <section class="settings-card settings-automation-summary" aria-label="自动行为摘要">
      <header class="settings-card-header">
        <span class="settings-card-icon"><IconClockCircle /></span>
        <div>
          <strong>自动行为摘要</strong>
          <span>App 运行时按下列计划自动执行；关闭开关即刻停止。</span>
        </div>
      </header>
      <ul class="settings-automation-summary-list">
        <li v-for="item in automationSummary" :key="item.key" :class="{ 'is-off': !item.on }">
          <span class="settings-automation-summary-state">{{ item.on ? "开" : "关" }}</span>
          <strong>{{ item.label }}</strong>
          <span>{{ item.detail }}</span>
        </li>
      </ul>
    </section>

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
        <div
          class="settings-setting-row settings-setting-row-field"
          :class="{ disabled: !settings.autoRefreshEnabled }"
        >
          <div class="settings-setting-copy">
            <strong>失败后重试</strong>
            <span>失败后按 1 分钟、2 分钟递增间隔重试，不改动正常刷新节奏。0 表示不重试。</span>
          </div>
          <a-input-number
            v-model="settings.refreshRetryCount"
            :min="0"
            :max="3"
            :step="1"
            :disabled="!settings.autoRefreshEnabled"
            aria-label="刷新失败后重试次数"
          >
            <template #suffix>次</template>
          </a-input-number>
        </div>
        <div
          class="settings-setting-row"
          :class="{ disabled: !settings.autoRefreshEnabled }"
        >
          <div class="settings-setting-copy">
            <strong>错开刷新时间</strong>
            <span>多个站点在几分钟内随机展开，避免同一秒集中打满网络。</span>
          </div>
          <a-switch
            v-model="settings.refreshJitter"
            :disabled="!settings.autoRefreshEnabled"
            aria-label="错开刷新时间"
          />
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
    <section class="settings-card">
      <header class="settings-card-header">
        <span class="settings-card-icon settings-card-icon-amber"><IconHistory /></span>
        <div>
          <strong>历史记录</strong>
          <span>只影响本地展示的明细，累计消耗统计不受限制。</span>
        </div>
      </header>

      <div class="settings-setting-list">
        <div class="settings-setting-row settings-setting-row-field">
          <div class="settings-setting-copy">
            <strong>测活记录保留</strong>
            <span>每个站点保留最近多少条测活明细。</span>
          </div>
          <a-input-number
            v-model="settings.livenessRecordLimit"
            :min="10"
            :max="500"
            :step="10"
            aria-label="测活记录保留条数"
          >
            <template #suffix>条</template>
          </a-input-number>
        </div>
        <div class="settings-setting-row settings-setting-row-field">
          <div class="settings-setting-copy">
            <strong>签到记录保留</strong>
            <span>按天去重后保留的天数，调小后旧记录会在保存时清理。</span>
          </div>
          <a-input-number
            v-model="settings.checkInRecordLimit"
            :min="30"
            :max="3650"
            :step="30"
            aria-label="签到记录保留天数"
          >
            <template #suffix>天</template>
          </a-input-number>
        </div>
      </div>
    </section>
    <SettingsLivenessSection
      v-if="livenessAvailable"
      :settings="settings"
      :liveness-model-options="livenessModelOptions"
      :selected-liveness-model-providers="selectedLivenessModelProviders"
    />
  </div>
</template>
