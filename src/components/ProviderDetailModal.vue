<script setup lang="ts">
import { computed } from "vue";
import {
  IconCalendar,
  IconCloud,
  IconEdit,
  IconRefresh,
} from "@arco-design/web-vue/es/icon";
import AccountQuotaBar from "./oauth/AccountQuotaBar.vue";
import AgentCliIcon from "./AgentCliIcon.vue";
import { useCliRuntimeStore } from "../stores/cli-runtime";
import type {
  AgentCliKind,
  AppSettings,
  LivenessRecord,
  Provider,
} from "../stores/providers";
import {
  providerCheckedInToday,
  supportsCheckIn,
  supportsRequestLogs,
  supportsUsage,
} from "../utils/provider-actions";
import {
  formatProviderSyncTime,
  providerApiKeyDisplayName,
  providerAuthModeDescription,
  providerCardStatusLabel,
  providerCardStatusTitle,
  providerCardStatusTone,
  providerDefaultApiKeyOption,
  providerDisplayLabel,
  providerModelScopeLabel,
  providerQuotaBarWindow,
  providerQuotaHeadline,
  providerQuotaScopeLabel,
  providerQuotaWindowRows,
  type ProviderCardTone,
  type ProviderQuotaWindowRow,
} from "../utils/provider-display";
import { latestAgentLivenessRecord, livenessTimestamp, providerLivenessEnabled, providerLivenessRecords, providerLivenessScope } from "../utils/provider-liveness";
import { providerCapabilitySourceLabel, providerOptionalCapabilityLabel } from "../utils/provider-capabilities";
import { providerScopeNoun } from "../utils/provider-protocol";
import { agentCliLabel } from "../utils/cli-environment";
import { formatDuration } from "../utils/duration";

/// 卡片点击后的只读详情面：把分散在额度、模型、测活、能力台账和 Agent
/// 配置里的同一份事实收进一页。编辑和签到仍是独立动作。
const props = defineProps<{
  visible: boolean;
  provider: Provider | null;
  settings: AppSettings;
}>();

const emit = defineEmits<{
  "update:visible": [visible: boolean];
  edit: [provider: Provider];
  refresh: [provider: Provider];
  checkIn: [provider: Provider];
  probeCapabilities: [provider: Provider];
  openAvailableModels: [provider: Provider];
  openUsage: [provider: Provider];
  openRequestLogs: [provider: Provider];
  openCheckInRecords: [provider: Provider];
  openLivenessDetails: [provider: Provider];
  openApiKeyManager: [provider: Provider];
  switchCliConfig: [provider: Provider, cliKind: AgentCliKind, model: string];
  launchTemporaryCli: [provider: Provider];
}>();

const store = useCliRuntimeStore();

const scopeNoun = computed(() =>
  props.provider ? providerScopeNoun(props.provider.identity.protocol) : "中转站",
);

const tone = computed<ProviderCardTone>(() => {
  const provider = props.provider;
  if (!provider) return "ok";
  return provider.runtime.status === "syncing"
    ? "syncing"
    : providerCardStatusTone(provider);
});
/// warning 按中转站卡自己的语义说话：这是「待签到」，只有带异常消息时才
/// 是「需关注」；详情里沿用同一句，避免和卡片标不一致。
const statusLabel = computed(() =>
  providerCardStatusLabel(
    tone.value,
    props.provider?.runtime.errorMessage ? "需关注" : "待签到",
  ),
);
const statusTitle = computed(() =>
  props.provider ? providerCardStatusTitle(props.provider) : "",
);

const accountLabel = computed(() => {
  const provider = props.provider;
  if (!provider) return "";
  return (
    provider.identity.displayName?.trim() ||
    provider.identity.username?.trim() ||
    provider.auth.apiUser?.trim() ||
    provider.auth.loginUsername?.trim() ||
    ""
  );
});

const keyLabel = computed(() => {
  const provider = props.provider;
  if (!provider) return "";
  const option = providerDefaultApiKeyOption(provider);
  return option ? providerApiKeyDisplayName(option) : "站点默认 Key";
});

const quotaHeadline = computed(() =>
  props.provider
    ? providerQuotaHeadline(props.provider)
    : { label: "", value: "", title: "" },
);
const quotaScope = computed(() =>
  props.provider ? providerQuotaScopeLabel(props.provider) : "",
);
const quotaRows = computed<ProviderQuotaWindowRow[]>(() =>
  props.provider ? providerQuotaWindowRows(props.provider, Date.now(), 12) : [],
);

const modelSummary = computed(() => {
  const provider = props.provider;
  if (!provider) return "";
  const visible = provider.visibleModels?.length ?? 0;
  const total = provider.capabilities.availableModels?.length ?? 0;
  const scope = providerModelScopeLabel(
    provider.capabilities.availableModelsState?.scope ??
      provider.actions.models.scope,
  );
  if (!total) return "尚未读取模型列表";
  return `${visible} / ${total} 个 · ${scope}`;
});

const pricingSummary = computed(() => {
  const pricing = props.provider?.capabilities.pricing;
  if (!pricing) return "暂无站点价目";
  const time = formatProviderSyncTime(pricing.updatedAt);
  if (pricing.error?.trim()) {
    return time ? `读取失败，沿用 ${time} 的读数` : "读取失败";
  }
  return time ? `更新于 ${time}` : "尚未读取";
});

const probeSummary = computed(() => {
  const provider = props.provider;
  if (!provider) return "";
  const time = formatProviderSyncTime(provider.capabilities.probedAt);
  if (provider.capabilities.errorMessage?.trim()) {
    return time ? `最近一次失败 · ${time}` : "最近一次失败";
  }
  return time ? `最近一次 ${time}` : "尚未探测";
});

const syncSummary = computed(() => {
  const provider = props.provider;
  if (!provider) return "";
  const time = formatProviderSyncTime(provider.automation.lastSyncedAt);
  return time || "尚未同步";
});

interface BindingRow {
  kind: AgentCliKind;
  label: string;
  keyLabel: string;
  model: string;
  serving: boolean;
  record: LivenessRecord | null;
  healthLabel: string;
}

function recordHealth(record: LivenessRecord | null) {
  if (!record) return "暂无测活记录";
  const time = formatProviderSyncTime(record.checkedAt) || "时间未知";
  return `${record.ok ? "成功" : "失败"} · ${Math.round(record.latencyMs)} ms · ${time}`;
}

/// 绑定行同时读两处：本机 CLI 配置（授权已经写进谁）和 per-Agent 模型绑定
/// （写了哪个模型）。只有配置没有模型时也要显示，否则用户会以为没绑。
const bindings = computed<BindingRow[]>(() => {
  const provider = props.provider;
  if (!provider) return [];
  const configs = new Map(
    store.cliRuntime.configs
      .filter((config) => config.providerId === provider.identity.id)
      .map((config) => [config.cliKind, config]),
  );
  const kinds = new Set<AgentCliKind>(configs.keys());
  for (const [kind, model] of Object.entries(provider.cli.agentModels ?? {})) {
    if (model?.trim()) kinds.add(kind as AgentCliKind);
  }
  return [...kinds].map((kind) => {
    const config = configs.get(kind);
    const option = config?.apiKeyLocalId?.trim()
      ? provider.auth.apiKeyOptions.find(
          (item) => item.localId.trim() === config.apiKeyLocalId?.trim(),
        )
      : providerDefaultApiKeyOption(provider);
    const model =
      provider.cli.agentModels?.[kind]?.trim() ||
      provider.cli.preferredModel?.trim() ||
      "";
    const baseUrl =
      provider.liveness.agentBaseUrls?.[kind]?.trim() || provider.identity.baseUrl;
    const record = latestAgentLivenessRecord(provider, kind, model, baseUrl);
    return {
      kind,
      label: agentCliLabel(store.cliEnvironmentProbe, kind),
      keyLabel: option ? providerApiKeyDisplayName(option) : "站点默认 Key",
      model,
      serving: Boolean(config),
      record,
      healthLabel: recordHealth(record),
    };
  });
});

/// 详情面拿得到全局设置，必须按后端实际使用的目标统计：卡片声明为空时会
/// 回落到全局 Agent 和模型，不能把其他模型的旧记录算进当前目标。
const livenessScope = computed(() =>
  props.provider
    ? providerLivenessScope(props.provider, props.settings)
    : null,
);

const livenessSummary = computed(() => {
  const provider = props.provider;
  if (!provider) return "";
  const records = providerLivenessRecords(provider, props.settings);
  if (!records.length) {
    return (provider.liveness.records?.length ?? 0) > 0
      ? "当前目标暂无测活记录"
      : "暂无测活记录";
  }
  const recent = [...records]
    .sort((left, right) => livenessTimestamp(right) - livenessTimestamp(left))
    .slice(0, 24);
  const ok = recent.filter((record) => record.ok).length;
  const time = formatProviderSyncTime(recent[0]?.checkedAt) || "时间未知";
  return `近 ${recent.length} 次成功 ${ok} 次 · 最近 ${time}`;
});

const capabilityRows = computed(() =>
  (props.provider?.capabilities.optional ?? []).map((state) => {
    const checked = formatProviderSyncTime(state.lastCheckedAt);
    const success = formatProviderSyncTime(state.lastSuccessAt);
    const source = providerCapabilitySourceLabel(state.source);
    return {
      key: state.capability,
      label: providerOptionalCapabilityLabel(state.capability),
      status: state.known ? (state.supported ? "可用" : "不支持") : "未确认",
      tone: state.known ? (state.supported ? "ok" : "off") : "unknown",
      meta: [source, checked ? `检查于 ${checked}` : "", success ? `最近成功 ${success}` : ""]
        .filter(Boolean)
        .join(" · "),
      detail:
        state.reason?.trim() ||
        state.error?.trim() ||
        "",
    };
  }),
);

const automationFacts = computed(() => {
  const provider = props.provider;
  if (!provider) return [];
  const settings = props.settings;
  const refresh = provider.automation.refreshInterval > 0
    ? provider.automation.refreshInterval
    : settings.refreshInterval;
  const facts = [
    {
      label: "自动刷新",
      value: settings.autoRefreshEnabled
        ? `每 ${formatDuration(refresh)}${provider.automation.refreshInterval > 0 ? "（本卡单独设置）" : "（跟随全局）"}`
        : "全局已关闭，仅手动刷新",
    },
    {
      label: "自动签到",
      value: supportsCheckIn(provider)
        ? settings.autoCheckInEnabled
          ? `每天 ${provider.automation.checkInTime || settings.checkInTime || "00:00"}${providerCheckedInToday(provider) ? " · 今天已签到" : ""}`
          : "未开启自动签到"
        : "站点不支持签到",
    },
  ];
  const livenessOn = providerLivenessEnabled(provider, settings);
  const livenessScope = provider.liveness.useGlobal
    ? {
        intervalMode: settings.livenessIntervalMode,
        interval: settings.livenessInterval,
        randomMinInterval: settings.livenessRandomMinInterval,
        randomMaxInterval: settings.livenessRandomMaxInterval,
      }
    : provider.liveness;
  const interval = livenessScope.intervalMode === "fixed"
    ? formatDuration(livenessScope.interval)
    : `${formatDuration(livenessScope.randomMinInterval)}–${formatDuration(livenessScope.randomMaxInterval)}`;
  const { cliKind: kind, model } = providerLivenessScope(provider, settings);
  const nextAt = formatProviderSyncTime(provider.liveness.nextAt);
  facts.push({
    label: "自动测活",
    value: livenessOn
      ? `每 ${interval} · ${agentCliLabel(store.cliEnvironmentProbe, kind)}${model ? ` · ${model}` : ""}${nextAt ? ` · 下次 ${nextAt}` : ""}`
      : "未启用",
  });
  return facts;
});

const cooldownLabel = computed(() => {
  const until = props.provider?.runtime.cooldownUntil;
  if (!until) return "";
  const date = new Date(until * 1000);
  return Number.isNaN(date.getTime())
    ? ""
    : date.toLocaleString("zh-CN", { hour12: false });
});

function run(action: (provider: Provider) => void) {
  if (props.provider) action(props.provider);
}

function runWithModel(
  action: (provider: Provider, kind: AgentCliKind, model: string) => void,
  kind: AgentCliKind,
  model: string,
) {
  if (props.provider) action(props.provider, kind, model);
}
</script>

<template>
  <a-modal
    :visible="visible"
    :width="860"
    modal-class="surface-modal provider-detail-modal"
    :footer="false"
    unmount-on-close
    @update:visible="emit('update:visible', $event)"
  >
    <template #title>
      <div class="surface-modal-title provider-detail-title">
        <span class="surface-modal-title-icon"><IconCloud /></span>
        <span class="surface-modal-title-copy">
          <strong>{{ provider ? providerDisplayLabel(provider) : '中转站详情' }}</strong>
        </span>
        <span v-if="provider" class="provider-detail-status" :class="`is-${tone}`">
          {{ statusLabel }}
        </span>
      </div>
    </template>

    <div v-if="provider" class="provider-detail">
      <header class="provider-detail-hero">
        <div class="provider-detail-identity">
          <span class="provider-detail-scope">{{ scopeNoun }}</span>
          <span class="provider-detail-protocol">{{ provider.protocolLabel }}</span>
          <span class="provider-detail-protocol">{{ providerAuthModeDescription(provider) }}</span>
          <span v-if="accountLabel" class="provider-detail-account">{{ accountLabel }}</span>
        </div>
        <p class="provider-detail-endpoint" :title="provider.identity.baseUrl">
          {{ provider.identity.baseUrl }}
        </p>
        <div class="provider-detail-actions">
          <a-button size="small" type="primary" @click="run((item) => emit('edit', item))">
            <template #icon><IconEdit /></template>
            编辑
          </a-button>
          <a-button
            size="small"
            :loading="provider.runtime.status === 'syncing'"
            :disabled="!provider.runtime.enabled"
            @click="run((item) => emit('refresh', item))"
          >
            <template #icon><IconRefresh /></template>
            刷新
          </a-button>
          <a-button
            v-if="provider.actions.temporaryCli"
            size="small"
            @click="run((item) => emit('launchTemporaryCli', item))"
          >启动临时 CLI</a-button>
        </div>
      </header>

      <section class="provider-detail-section">
        <header class="provider-detail-section-head">
          <h3>当前状态</h3>
          <span v-if="statusTitle" class="provider-detail-error">{{ statusTitle }}</span>
        </header>
        <dl class="provider-detail-facts">
          <div>
            <dt>可用额度</dt>
            <dd>
              <strong>{{ quotaHeadline.value }}</strong>
              <small>{{ quotaHeadline.label }} · {{ quotaScope }}</small>
            </dd>
          </div>
          <div>
            <dt>余额读取</dt>
            <dd>
              <strong>{{ syncSummary }}</strong>
              <small>最近一次成功同步额度的时间</small>
            </dd>
          </div>
          <div>
            <dt>模型</dt>
            <dd>
              <strong>{{ modelSummary }}</strong>
              <small>{{ keyLabel }}</small>
            </dd>
          </div>
          <div>
            <dt>价格</dt>
            <dd>
              <strong>{{ pricingSummary }}</strong>
              <small>站点价目优先于公开参考价</small>
            </dd>
          </div>
          <div>
            <dt>测活</dt>
            <dd>
              <strong>{{ livenessSummary }}</strong>
              <small>{{ livenessScope?.model || "默认模型" }}</small>
            </dd>
          </div>
          <div>
            <dt>能力探测</dt>
            <dd>
              <strong>{{ probeSummary }}</strong>
              <small>逐接口结论见下方台账</small>
            </dd>
          </div>
        </dl>
        <div v-if="quotaRows.length" class="provider-detail-quota">
          <AccountQuotaBar
            v-for="row in quotaRows"
            :key="row.label"
            :window="providerQuotaBarWindow(row)"
            compact
          />
        </div>
      </section>

      <section class="provider-detail-section">
        <header class="provider-detail-section-head">
          <h3>Agent 绑定</h3>
          <span class="provider-detail-hint">本机默认配置把哪个 Key 和模型写给了谁</span>
        </header>
        <p v-if="!bindings.length" class="provider-detail-empty">
          还没有 Agent 使用这{{ scopeNoun }}的授权。可从卡片底栏切换过去。
        </p>
        <ul v-else class="provider-detail-bindings">
          <li v-for="binding in bindings" :key="binding.kind">
            <AgentCliIcon :kind="binding.kind" :size="18" />
            <div class="provider-detail-binding-copy">
              <strong>{{ binding.label }}</strong>
              <span>{{ binding.keyLabel }}<template v-if="binding.model"> · {{ binding.model }}</template></span>
            </div>
            <span
              class="provider-detail-binding-health"
              :class="{ 'is-bad': binding.record && !binding.record.ok }"
            >{{ binding.healthLabel }}</span>
            <span v-if="!binding.serving" class="provider-detail-chip">仅模型绑定</span>
            <a-button
              size="mini"
              type="outline"
              @click="runWithModel((item, kind, model) => emit('switchCliConfig', item, kind, model), binding.kind, binding.model)"
            >{{ binding.serving ? "重新写入" : "切换到此" }}</a-button>
          </li>
        </ul>
      </section>

      <section class="provider-detail-section">
        <header class="provider-detail-section-head">
          <h3>能力台账</h3>
          <span class="provider-detail-hint">每个站点接口最近一次结论与检查时间</span>
        </header>
        <p v-if="!capabilityRows.length" class="provider-detail-empty">
          尚未探测站点能力。
        </p>
        <ul v-else class="provider-detail-capabilities">
          <li v-for="row in capabilityRows" :key="row.key" :class="`is-${row.tone}`">
            <strong>{{ row.label }}</strong>
            <span class="provider-detail-chip">{{ row.status }}</span>
            <small>{{ row.meta }}</small>
            <small v-if="row.detail" class="provider-detail-capability-detail">{{ row.detail }}</small>
          </li>
        </ul>
      </section>

      <section class="provider-detail-section">
        <header class="provider-detail-section-head">
          <h3>自动化</h3>
          <span v-if="provider.runtime.cooldownUntil" class="provider-detail-error">
            冷却至 {{ cooldownLabel }}
          </span>
        </header>
        <dl class="provider-detail-facts is-compact">
          <div v-for="fact in automationFacts" :key="fact.label">
            <dt>{{ fact.label }}</dt>
            <dd><strong>{{ fact.value }}</strong></dd>
          </div>
        </dl>
      </section>

      <section class="provider-detail-section">
        <header class="provider-detail-section-head">
          <h3>更多入口</h3>
        </header>
        <div class="provider-detail-entries">
          <a-button size="small" @click="run((item) => emit('openAvailableModels', item))">可用模型</a-button>
          <a-button v-if="supportsUsage(provider)" size="small" @click="run((item) => emit('openUsage', item))">用量趋势</a-button>
          <a-button v-if="supportsRequestLogs(provider)" size="small" @click="run((item) => emit('openRequestLogs', item))">请求日志</a-button>
          <a-button size="small" @click="run((item) => emit('probeCapabilities', item))">能力探测</a-button>
          <a-button v-if="provider.liveness.records?.length" size="small" @click="run((item) => emit('openLivenessDetails', item))">测活明细</a-button>
          <a-button v-if="supportsCheckIn(provider)" size="small" @click="run((item) => emit('openCheckInRecords', item))">签到记录</a-button>
          <a-button v-if="provider.actions.apiKeyManagement" size="small" @click="run((item) => emit('openApiKeyManager', item))">API Key 管理</a-button>
          <a-button
            v-if="supportsCheckIn(provider)"
            size="small"
            :disabled="provider.actions.checkedInToday || !provider.runtime.enabled"
            @click="run((item) => emit('checkIn', item))"
          >
            <template #icon><IconCalendar /></template>
            {{ provider.actions.checkedInToday ? "今天已签到" : "立即签到" }}
          </a-button>
        </div>
      </section>
    </div>
  </a-modal>
</template>
