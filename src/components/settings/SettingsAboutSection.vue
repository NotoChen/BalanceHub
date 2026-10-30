<script setup lang="ts">
import { computed } from "vue";
import {
  IconBug,
  IconFolder,
  IconInfoCircle,
  IconLink,
  IconRefresh,
  IconSync,
} from "@arco-design/web-vue/es/icon";
import { openDataDirectory, openLogDirectory, openProjectRepository } from "../../api/app";
import type { AppSettings, LogLevel } from "../../stores/providers";
import { formatAppVersionLabel } from "../../utils/app-version";

const props = defineProps<{
  settings: AppSettings;
  appVersion: string;
  checkingForUpdate: boolean;
}>();

const emit = defineEmits<{ "check-for-update": [] }>();

const logLevelOptions: { label: string; value: LogLevel }[] = [
  { label: "仅错误", value: "error" },
  { label: "警告", value: "warn" },
  { label: "常规", value: "info" },
  { label: "调试", value: "debug" },
];

/// 从「告警」到「调试」是逐级放大，用一句提示说明代价，避免用户随手拉满。
const logLevelHint = computed(() => {
  switch (props.settings.logLevel) {
    case "error":
      return "只记录失败，日志最干净。";
    case "warn":
      return "记录失败和异常回退，适合长期开启。";
    case "debug":
      return "记录排查细节，文件增长较快，排查完建议调回常规。";
    default:
      return "记录关键后台事件，日常使用推荐。";
  }
});

const updateIntervalHint = computed(() => {
  const hours = props.settings.updateCheckIntervalHours;
  return hours >= 24 && hours % 24 === 0
    ? `每 ${hours / 24} 天检查一次`
    : `每 ${hours} 小时检查一次`;
});

function openRepository() {
  void openProjectRepository();
}

function openLogs() {
  void openLogDirectory();
}

function openData() {
  void openDataDirectory();
}
</script>

<template>
  <div class="settings-page settings-about-page">
    <section class="settings-card">
      <header class="settings-card-header">
        <span class="settings-card-icon"><IconSync /></span>
        <div>
          <strong>版本更新</strong>
          <span>自动检查只提示新版本，不会在后台静默安装。</span>
        </div>
        <span class="settings-version-badge">{{ formatAppVersionLabel(appVersion) }}</span>
      </header>

      <div class="settings-setting-list">
        <div class="settings-setting-row">
          <div class="settings-setting-copy">
            <strong>自动检查更新</strong>
            <span>启动后和运行期间按间隔检查，发现新版本时提示。</span>
          </div>
          <a-switch v-model="settings.updateAutoCheck" aria-label="自动检查更新" />
        </div>
        <div
          class="settings-setting-row settings-setting-row-field"
          :class="{ disabled: !settings.updateAutoCheck }"
        >
          <div class="settings-setting-copy">
            <strong>检查间隔</strong>
            <span>{{ updateIntervalHint }}</span>
          </div>
          <a-input-number
            v-model="settings.updateCheckIntervalHours"
            :min="1"
            :max="168"
            :step="1"
            :disabled="!settings.updateAutoCheck"
            aria-label="检查更新间隔（小时）"
          >
            <template #suffix>小时</template>
          </a-input-number>
        </div>
        <div class="settings-setting-row">
          <div class="settings-setting-copy">
            <strong>立即检查</strong>
            <span>手动检查不受自动检查开关影响。</span>
          </div>
          <a-button :loading="checkingForUpdate" @click="emit('check-for-update')">
            <template #icon><IconRefresh /></template>检查更新
          </a-button>
        </div>
      </div>
    </section>

    <section class="settings-card">
      <header class="settings-card-header">
        <span class="settings-card-icon settings-card-icon-amber"><IconBug /></span>
        <div>
          <strong>诊断与日志</strong>
          <span>后台任务出错时，日志文件是排查唯一线索。</span>
        </div>
      </header>

      <div class="settings-setting-list">
        <div class="settings-setting-row settings-setting-row-field">
          <div class="settings-setting-copy">
            <strong>日志级别</strong>
            <span>{{ logLevelHint }}</span>
          </div>
          <a-radio-group v-model="settings.logLevel" type="button" size="small">
            <a-radio v-for="option in logLevelOptions" :key="option.value" :value="option.value">
              {{ option.label }}
            </a-radio>
          </a-radio-group>
        </div>
        <div class="settings-setting-row settings-setting-row-field">
          <div class="settings-setting-copy">
            <strong>日志保留</strong>
            <span>按天滚动，超过保留期的旧文件自动删除。</span>
          </div>
          <a-input-number
            v-model="settings.logRetentionDays"
            :min="1"
            :max="90"
            :step="1"
            aria-label="日志保留天数"
          >
            <template #suffix>天</template>
          </a-input-number>
        </div>
        <div class="settings-setting-row">
          <div class="settings-setting-copy">
            <strong>打开日志目录</strong>
            <span>出问题时把当天日志文件发出来即可。</span>
          </div>
          <a-button @click="openLogs">
            <template #icon><IconFolder /></template>打开
          </a-button>
        </div>
        <div class="settings-setting-row">
          <div class="settings-setting-copy">
            <strong>打开数据目录</strong>
            <span>临时 CLI、隔离登录环境等运行数据都在这里。</span>
          </div>
          <a-button @click="openData">
            <template #icon><IconFolder /></template>打开
          </a-button>
        </div>
      </div>
    </section>

    <section class="settings-card">
      <header class="settings-card-header">
        <span class="settings-card-icon"><IconInfoCircle /></span>
        <div>
          <strong>关于 BalanceHub</strong>
          <span>本地桌面应用，只读取中转站自身的接口。</span>
        </div>
      </header>
      <div class="settings-setting-row">
        <div class="settings-setting-copy">
          <strong>问题反馈与更新说明</strong>
          <span>仓库只接受 Issue，请附上版本号和日志片段。</span>
        </div>
        <a-button @click="openRepository">
          <template #icon><IconLink /></template>打开 GitHub
        </a-button>
      </div>
    </section>
  </div>
</template>
