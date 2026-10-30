<script setup lang="ts">
import { IconCalendarClock, IconHistory, IconRefresh } from "@arco-design/web-vue/es/icon";
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
      :settings="settings"
      :liveness-model-options="livenessModelOptions"
      :selected-liveness-model-providers="selectedLivenessModelProviders"
    />
  </div>
</template>
