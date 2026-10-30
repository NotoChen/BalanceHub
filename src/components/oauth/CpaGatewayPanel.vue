<script setup lang="ts">
import { computed, onScopeDispose, ref, watch } from "vue";
import {
  Activity,
  ChevronDown,
  Download,
  Gauge,
  RefreshCw,
  Settings2,
  Search,
  MoreHorizontal,
  FileCode2,
  PencilLine,
  Plus,
  Puzzle,
  RotateCw,
  ScrollText,
  TimerOff,
  Trash2,
  TriangleAlert,
} from "@lucide/vue";
import { Message } from "@arco-design/web-vue";
import type { Provider } from "../../stores/provider-types";
import type {
  CpaAccountSummary,
  CpaCredentialFile,
  OAuthImportPreview,
} from "../../stores/oauth-types";
import {
  checkCpaVersion,
  previewCpaImport,
  setCpaDisabled,
  updateCpaFields,
  refreshCpaAccount,
  resetCpaCooldown,
  refreshAllCpa,
  deleteCpaAccount,
  viewCpaAccountFile,
} from "../../api/oauth-accounts";
import { useProviderStore } from "../../stores/providers";
import {
  accountTimestamp,
  managedModelCount,
  quotaNumber,
} from "../../utils/oauth-display";
import { confirmAction } from "../../composables/provider-credential-dialogs";
import ContentEditor from "../ContentEditor.vue";
import FileEditorModal from "../FileEditorModal.vue";
import CpaSchedulingSection from "./CpaSchedulingSection.vue";
import CpaModelRulesSection from "./CpaModelRulesSection.vue";
import CpaClientKeysSection from "./CpaClientKeysSection.vue";
import CpaPluginsDialog from "./CpaPluginsDialog.vue";
import CpaLoginDialog from "./CpaLoginDialog.vue";
import CpaAccountQuota from "./CpaAccountQuota.vue";
import CpaRequestLogDialog from "./CpaRequestLogDialog.vue";
import CpaErrorLogDialog from "./CpaErrorLogDialog.vue";
import CpaUsageDialog from "./CpaUsageDialog.vue";
const props = defineProps<{ provider: Provider }>();
const emit = defineEmits<{
  edit: [];
  models: [];
  import: [preview: OAuthImportPreview];
}>();
const store = useProviderStore();
const selected = ref<string[]>([]);
const search = ref("");
const busy = ref(false);
const error = ref("");
const pendingName = ref<string | null>(null);
const expanded = ref<string | null>(null);
const viewer = ref<CpaCredentialFile | null>(null);
const viewerOpen = ref(false);
const viewerLoading = ref(false);
const refreshingAll = ref(false);
const logsOpen = ref(false);
const pluginsOpen = ref(false);
const loginOpen = ref(false);
const versionChecking = ref(false);
const requestLogsOpen = ref(false);
const usageOpen = ref(false);
interface FieldDraft {
  name: string;
  authIndex: string;
  priority: number | undefined;
  weight: number | undefined;
  requestRetry: number | undefined;
  note: string;
  excludedModels: string[];
}
const editor = ref<FieldDraft | null>(null);
const editorVisible = ref(false);
const editorSaving = ref(false);
const editorError = ref("");
let disposed = false;
onScopeDispose(() => {
  disposed = true;
});
const gateway = computed(() => props.provider.capabilities.gateway);
const modelCount = computed(() => managedModelCount(props.provider));
const requestTotals = computed(() => {
  const total = gateway.value?.totalRequests ?? 0;
  const failed = gateway.value?.failedRequests ?? 0;
  return {
    total,
    failed,
    rate: total > 0 ? Math.round(((total - failed) / total) * 1000) / 10 : null,
  };
});
const revision = computed(() => props.provider.auth.credentialRevision || 0);
/// Meter scale for the success-rate tile; the tile only renders with traffic.
const rateScale = computed(() => (requestTotals.value.rate ?? 0) / 100);
watch(
  () => gateway.value?.accounts,
  (accounts) => {
    selected.value = selected.value.filter((name) =>
      accounts?.some((account) => account.name === name && account.canDownload),
    );
  },
);
const accounts = computed(() =>
  (gateway.value?.accounts || []).filter((a) =>
    `${a.email} ${a.name} ${a.provider}`
      .toLowerCase()
      .includes(search.value.toLowerCase()),
  ),
);
/// The expanded row owns one small bar chart; each column is one bucket,
/// stacked as success under failures so both volume and errors read at once.
const recentTrend = computed(() => {
  const account = accounts.value.find((a) => a.name === expanded.value);
  const buckets = account?.recentRequests.slice(-8) || [];
  if (!buckets.length) return null;
  const peak = Math.max(1, ...buckets.map((b) => b.success + b.failed));
  return {
    buckets: buckets.map((b) => {
      const total = b.success + b.failed;
      return {
        ...b,
        height: total ? Math.max(4, Math.round((total / peak) * 100)) : 0,
        failedShare: total ? Math.round((b.failed / total) * 100) : 0,
      };
    }),
  };
});
function select(name: string, checked: boolean | string | number) {
  selected.value = checked
    ? [...new Set([...selected.value, name])]
    : selected.value.filter((v) => v !== name);
}
function locked(name?: string) {
  return (
    busy.value ||
    refreshingAll.value ||
    (!!pendingName.value && pendingName.value !== name)
  );
}
async function reload() {
  const failure = await store.refreshByIds([props.provider.identity.id]);
  if (!disposed && failure) error.value = failure;
}
async function refresh() {
  if (busy.value || pendingName.value) return;
  busy.value = true;
  error.value = "";
  try {
    await reload();
  } catch (cause) {
    if (!disposed) error.value = String(cause);
  } finally {
    busy.value = false;
  }
}
async function importSelected() {
  if (!selected.value.length || busy.value || pendingName.value) return;
  busy.value = true;
  error.value = "";
  try {
    const preview = await previewCpaImport(
      props.provider.identity.id,
      selected.value,
    );
    if (disposed) {
      const { discardOAuthImport } = await import("../../api/oauth-accounts");
      void discardOAuthImport(preview.token).catch(() => {});
    } else emit("import", preview);
  } catch (cause) {
    if (!disposed) error.value = String(cause);
  } finally {
    busy.value = false;
  }
}
async function toggle(account: CpaAccountSummary) {
  if (locked()) return;
  const id = props.provider.identity.id;
  const current = revision.value;
  const { name, authIndex } = account;
  const disabled = !account.disabled;
  pendingName.value = name;
  try {
    const confirmed = await confirmAction(
      disabled ? "停用 CPA 授权" : "启用 CPA 授权",
      `这会${disabled ? "停止" : "允许"}所选 CPA 使用这份授权处理请求。`,
      disabled ? "停用远端授权" : "启用远端授权",
      "warning",
    );
    if (!confirmed || disposed || props.provider.identity.id !== id) return;
    error.value = "";
    await setCpaDisabled(id, name, authIndex, disabled, current);
    await reload();
  } catch (cause) {
    if (!disposed) error.value = String(cause);
  } finally {
    pendingName.value = null;
  }
}
function editFields(account: CpaAccountSummary) {
  if (locked()) return;
  editor.value = {
    name: account.name,
    authIndex: account.authIndex,
    priority: account.priority ?? undefined,
    weight: account.weight ?? undefined,
    requestRetry: account.requestRetry ?? undefined,
    note: account.note,
    excludedModels: [...account.excludedModels],
  };
  editorError.value = "";
  editorVisible.value = true;
}
async function saveFields() {
  const draft = editor.value;
  if (!draft || editorSaving.value) return;
  const id = props.provider.identity.id;
  const current = revision.value;
  editorSaving.value = true;
  editorError.value = "";
  try {
    await updateCpaFields(
      id,
      draft.name,
      draft.authIndex,
      {
        priority: draft.priority ?? null,
        weight: draft.weight ?? null,
        requestRetry: draft.requestRetry ?? null,
        note: draft.note,
        excludedModels: draft.excludedModels,
      },
      current,
    );
    if (disposed) return;
    editorVisible.value = false;
    editor.value = null;
    Message.success("CPA 调度字段已更新");
    await reload();
  } catch (cause) {
    if (!disposed) editorError.value = String(cause);
  } finally {
    editorSaving.value = false;
  }
}
async function refreshRemote(account: CpaAccountSummary) {
  if (locked()) return;
  const id = props.provider.identity.id;
  const current = revision.value;
  const name = account.name;
  pendingName.value = name;
  try {
    const confirmed = await confirmAction(
      "请求 CPA 续期",
      "CPA 将自行使用保存的刷新令牌换取新令牌，替换后旧令牌立即失效。仅当 CPA 是该账号唯一的续期方时执行。",
      "请求续期",
      "warning",
    );
    if (!confirmed || disposed || props.provider.identity.id !== id) return;
    error.value = "";
    await refreshCpaAccount(id, name, account.authIndex, current);
    Message.success("已向 CPA 请求续期，稍后刷新核对");
    await reload();
  } catch (cause) {
    if (!disposed) error.value = String(cause);
  } finally {
    pendingName.value = null;
  }
}
async function resetCooldown(account: CpaAccountSummary) {
  if (locked()) return;
  const id = props.provider.identity.id;
  const current = revision.value;
  const name = account.name;
  pendingName.value = name;
  try {
    const confirmed = await confirmAction(
      "重置 CPA 冷却",
      `将清除 ${account.email || name} 在 CPA 的冷却与退避状态，让网关可以再次调度这份授权。若额度确实已耗尽，请求会很快再次失败。`,
      "重置冷却",
      "warning",
    );
    if (!confirmed || disposed || props.provider.identity.id !== id) return;
    error.value = "";
    await resetCpaCooldown(id, name, account.authIndex, current);
    Message.success("已重置 CPA 冷却");
    await reload();
  } catch (cause) {
    if (!disposed) error.value = String(cause);
  } finally {
    pendingName.value = null;
  }
}
async function refreshAll() {
  if (busy.value || refreshingAll.value || pendingName.value) return;
  const id = props.provider.identity.id;
  const current = revision.value;
  refreshingAll.value = true;
  try {
    const confirmed = await confirmAction(
      "请求 CPA 更新全部授权",
      "CPA 会依次为所有授权换取新令牌，旧令牌立即失效。仅当 CPA 是这些账号唯一的续期方时执行。",
      "全部续期",
      "warning",
    );
    if (!confirmed || disposed || props.provider.identity.id !== id) return;
    error.value = "";
    await refreshAllCpa(id, current);
    Message.success("已向 CPA 请求全部续期，稍后刷新核对");
    await reload();
  } catch (cause) {
    if (!disposed) error.value = String(cause);
  } finally {
    refreshingAll.value = false;
  }
}
/// The gateway resolves the newest release through its own GitHub access, so
/// this answer can be unavailable even while everything else works.
async function checkVersion() {
  if (versionChecking.value) return;
  const id = props.provider.identity.id;
  versionChecking.value = true;
  try {
    const check = await checkCpaVersion(id);
    if (disposed) return;
    if (check.updateAvailable === true) {
      Message.info(`网关 ${check.current}，最新 ${check.latest}，可在网关主机升级`);
    } else if (check.updateAvailable === false) {
      Message.success(`网关 ${check.current} 已是最新`);
    } else {
      Message.info(`网关 ${check.current}，最新 ${check.latest}`);
    }
  } catch (cause) {
    if (!disposed) Message.error(String(cause));
  } finally {
    versionChecking.value = false;
  }
}
function openErrorLogs() {
  if (!locked()) logsOpen.value = true;
}
function closePlugins() {
  pluginsOpen.value = false;
}
/// The gateway holds the new account, so the card re-reads the inventory
/// instead of waiting for the next scheduled refresh.
async function loginCompleted() {
  loginOpen.value = false;
  Message.success("网关已完成授权");
  await reload();
}
async function removeRemote(account: CpaAccountSummary) {
  if (locked()) return;
  const id = props.provider.identity.id;
  const current = revision.value;
  const name = account.name;
  pendingName.value = name;
  try {
    const confirmed = await confirmAction(
      "从 CPA 移除授权",
      `将从 CPA 删除授权文件 ${name}。此操作不可撤销，使用该授权的请求会立即失败；BalanceHub 的本地副本不受影响。`,
      "删除远端授权",
      "danger",
    );
    if (!confirmed || disposed || props.provider.identity.id !== id) return;
    error.value = "";
    await deleteCpaAccount(id, name, account.authIndex, current);
    Message.success("已从 CPA 移除授权");
    await reload();
  } catch (cause) {
    if (!disposed) error.value = String(cause);
  } finally {
    pendingName.value = null;
  }
}
async function openFile(account: CpaAccountSummary) {
  if (viewerLoading.value) return;
  viewerLoading.value = true;
  error.value = "";
  viewer.value = null;
  viewerOpen.value = true;
  try {
    const file = await viewCpaAccountFile(
      props.provider.identity.id,
      account.name,
    );
    if (!disposed) viewer.value = file;
  } catch (cause) {
    if (!disposed) {
      viewerOpen.value = false;
      error.value = String(cause);
    }
  } finally {
    viewerLoading.value = false;
  }
}
function closeViewer() {
  viewerOpen.value = false;
  viewer.value = null;
}
function retryLabel(value: number | null) {
  return value == null ? "未设置" : value <= 0 ? "使用网关默认" : `${value} 次`;
}
function cooling(account: CpaAccountSummary) {
  return account.quotaExceeded || account.backoffLevel != null;
}
</script>
<template>
  <div class="cpa-gateway-panel">
    <div class="cpa-panel-toolbar">
      <span :title="provider.identity.baseUrl">{{
        provider.identity.baseUrl
      }}</span
      ><a-button size="small" @click="emit('edit')"
        ><template #icon><Settings2 :size="14" /></template>连接设置</a-button
      ><a-button
        size="small"
        :disabled="!!pendingName || refreshingAll"
        :loading="busy"
        @click="refresh"
        ><template #icon><RefreshCw :size="14" /></template>刷新</a-button
      ><a-button
        size="small"
        :disabled="!!pendingName || busy"
        :loading="refreshingAll"
        @click="refreshAll"
        ><template #icon><RotateCw :size="14" /></template>全部续期</a-button
      ><a-dropdown trigger="click" position="br">
        <a-button size="small" :disabled="!!pendingName || busy"
          ><template #icon><Activity :size="14" /></template>诊断<ChevronDown
            :size="12"
            class="cpa-toolbar-caret"
        /></a-button>
        <template #content>
          <a-doption @click="pluginsOpen = true"
            ><template #icon><Puzzle :size="13" /></template>插件</a-doption
          ><a-doption @click="requestLogsOpen = true"
            ><template #icon><ScrollText :size="13" /></template
            >请求日志</a-doption
          ><a-doption @click="usageOpen = true"
            ><template #icon><Gauge :size="13" /></template
            >调用用量</a-doption
          ><a-doption @click="openErrorLogs"
            ><template #icon><TriangleAlert :size="13" /></template
            >错误日志</a-doption
          ><a-doption :disabled="versionChecking" @click="checkVersion"
            ><template #icon><RefreshCw :size="13" /></template
            >检查网关更新</a-doption
          >
        </template></a-dropdown
      >
    </div>
    <a-alert v-if="error || provider.runtime.errorMessage" type="warning">{{
      error || provider.runtime.errorMessage
    }}</a-alert>
    <div class="cpa-overview">
      <div>
        <strong
          >{{ gateway?.accounts.filter((a) => a.available).length ?? "—"
          }}<small> / {{ gateway?.accounts.length ?? "—" }}</small></strong
        ><span>可用授权 / 全部</span>
      </div>
      <button type="button" @click="emit('models')">
        <strong>{{ modelCount ?? "—" }}</strong
        ><span>可用模型 ↗</span>
      </button>
      <div v-if="requestTotals.total > 0" class="cpa-overview-rate">
        <strong>{{ requestTotals.rate }}<small>%</small></strong>
        <div class="cpa-overview-meter" aria-hidden="true">
          <span :style="{ transform: `scaleX(${rateScale})` }" />
        </div>
        <span
          >{{ quotaNumber(requestTotals.total) }} 请求 ·
          {{ quotaNumber(requestTotals.failed) }} 失败</span
        >
      </div>
    </div>
    <CpaSchedulingSection :provider="provider" :revision="revision" />
    <CpaClientKeysSection :provider="provider" :revision="revision" />
    <CpaModelRulesSection :provider="provider" :revision="revision" />
    <div class="cpa-selection">
      <a-input v-model="search" allow-clear placeholder="搜索账号、服务或文件名"
        ><template #prefix><Search :size="14" /></template></a-input
      ><a-button
        :disabled="!!pendingName || busy"
        @click="loginOpen = true"
        ><template #icon><Plus :size="14" /></template>添加账号</a-button
      ><a-button
        :disabled="!selected.length || !!pendingName"
        :loading="busy"
        @click="importSelected"
        ><template #icon><Download :size="14" /></template>导入所选
        {{ selected.length || "" }}</a-button
      >
    </div>
    <div class="cpa-account-list" aria-label="CPA 授权列表">
      <div
        v-for="account in accounts"
        :key="account.authIndex || account.name"
        class="cpa-account-row"
        :class="{ expanded: expanded === account.name }"
      >
        <div class="cpa-account-main">
          <a-checkbox
            :model-value="selected.includes(account.name)"
            :disabled="!account.canDownload"
            :title="account.downloadUnavailableReason || undefined"
            :aria-label="`选择 ${account.email || account.name}`"
            @change="select(account.name, $event === true)"
          />
          <button
            type="button"
            class="cpa-account-copy"
            :aria-expanded="expanded === account.name"
            @click="
              expanded = expanded === account.name ? null : account.name
            "
          >
            <strong>{{ account.email || account.name }}</strong
            ><span
              >{{ account.provider || "未标明服务" }} · {{ account.name }}</span
            >
          </button>
          <span
            class="cpa-account-status"
            :class="{ unavailable: !account.available }"
            >{{ account.statusLabel }}</span
          ><span
            v-if="cooling(account)"
            class="cpa-account-cooldown"
            :title="account.quotaReason || 'CPA 正在退避调度该授权'"
            >冷却中</span
          ><a-switch
            :model-value="!account.disabled"
            :loading="pendingName === account.name"
            :disabled="!!pendingName || busy"
            :aria-label="`${account.disabled ? '启用' : '停用'}远端授权`"
            size="small"
            @change="toggle(account)"
          /><a-dropdown
            trigger="click"
            position="br"
            :disabled="!!pendingName || busy"
          >
            <a-button
              size="mini"
              type="text"
              :aria-label="`${account.name} 更多操作`"
              ><MoreHorizontal :size="15"
            /></a-button>
            <template #content>
              <a-doption @click="expanded = account.name"
                ><template #icon><Settings2 :size="13" /></template
                >展开详情</a-doption
              >
              <a-doption @click="editFields(account)"
                ><template #icon><PencilLine :size="13" /></template
                >编辑调度字段</a-doption
              >
              <a-doption @click="openFile(account)"
                ><template #icon><FileCode2 :size="13" /></template
                >查看授权文件</a-doption
              >
              <a-doption @click="refreshRemote(account)"
                ><template #icon><RotateCw :size="13" /></template
                >请求 CPA 续期</a-doption
              >
              <a-doption v-if="cooling(account)" @click="resetCooldown(account)"
                ><template #icon><TimerOff :size="13" /></template
                >重置冷却</a-doption
              >
              <a-doption
                style="color: rgb(var(--red-6))"
                @click="removeRemote(account)"
                ><template #icon><Trash2 :size="13" /></template
                >从 CPA 移除</a-doption
              >
            </template>
          </a-dropdown>
        </div>
        <div v-if="expanded === account.name" class="cpa-account-detail">
          <div class="cpa-detail-stats">
            <div>
              <strong>{{ quotaNumber(account.success) }}</strong
              ><span>成功请求</span>
            </div>
            <div>
              <strong>{{ quotaNumber(account.failed) }}</strong
              ><span>失败请求</span>
            </div>
            <div>
              <strong>{{ account.priority ?? "—" }}</strong
              ><span>优先级</span>
            </div>
            <div>
              <strong>{{ account.weight ?? "—" }}</strong
              ><span>权重</span>
            </div>
            <div>
              <strong>{{ retryLabel(account.requestRetry) }}</strong
              ><span>单账号重试</span>
            </div>
            <div>
              <strong>{{ accountTimestamp(account.lastRefresh) }}</strong
              ><span>最近续期</span>
            </div>
          </div>
          <div class="cpa-detail-models">
            <span
              v-for="model in account.models.slice(0, 6)"
              :key="model"
              class="cpa-model"
              >{{ model }}</span
            ><span v-if="!account.models.length" class="cpa-model empty"
              >CPA 未报告模型</span
            ><span v-if="account.models.length > 6" class="cpa-model empty"
              >还有 {{ account.models.length - 6 }} 个</span
            >
            <span
              v-for="model in account.excludedModels"
              :key="`excluded-${model}`"
              class="cpa-model excluded"
              :title="`已对 ${account.name} 排除`"
              >− {{ model }}</span
            >
          </div>
          <p v-if="account.note" class="cpa-detail-note">
            备注：{{ account.note }}
          </p>
          <p v-if="account.nextRetryAfter" class="cpa-detail-note">
            下次重试：{{ accountTimestamp(account.nextRetryAfter) }}
          </p>
          <p v-if="account.quotaExceeded" class="cpa-detail-note warning">
            冷却原因：{{ account.quotaReason || "配额已用尽" }}<template
              v-if="account.quotaNextRecoverAt"
            >
              · 预计 {{ accountTimestamp(account.quotaNextRecoverAt) }} 恢复</template
            >
          </p>
          <p v-else-if="account.backoffLevel != null" class="cpa-detail-note warning">
            退避等级 {{ account.backoffLevel }}，CPA 正在降低该授权的调度频率
          </p>
          <div v-if="recentTrend" class="cpa-detail-trend">
            <span class="cpa-detail-trend-label">最近请求</span>
            <div class="cpa-trend-bars">
              <div
                v-for="bucket in recentTrend.buckets"
                :key="bucket.time"
                class="cpa-trend-bar"
                :title="`${bucket.time} · 成功 ${bucket.success} · 失败 ${bucket.failed}`"
              >
                <div class="cpa-trend-columns">
                  <span
                    class="is-column"
                    :style="{ height: `${bucket.height}%` }"
                    ><i
                      v-if="bucket.failed"
                      class="is-failed"
                      :style="{ height: `${bucket.failedShare}%` }"
                  /></span>
                </div>
                <em>{{ bucket.time }}</em>
              </div>
            </div>
          </div>
          <CpaAccountQuota
            v-if="account.supportsQuota"
            :provider="provider"
            :account="account"
            :api-version="gateway?.apiVersion ?? ''"
          />
          <div class="cpa-detail-actions">
            <a-button size="mini" @click="openFile(account)"
              ><template #icon><FileCode2 :size="12" /></template>授权文件</a-button
            ><a-button size="mini" @click="editFields(account)"
              ><template #icon><PencilLine :size="12" /></template>编辑字段</a-button
            ><a-button size="mini" @click="refreshRemote(account)"
              ><template #icon><RotateCw :size="12" /></template>请求续期</a-button
            ><a-button
              v-if="cooling(account)"
              size="mini"
              @click="resetCooldown(account)"
              ><template #icon><TimerOff :size="12" /></template>重置冷却</a-button
            ><a-button size="mini" status="danger" @click="removeRemote(account)"
              ><template #icon><Trash2 :size="12" /></template>移除</a-button
            >
          </div>
        </div>
      </div>
      <p v-if="!accounts.length" class="cpa-empty">
        {{ gateway ? "没有符合条件的授权" : "刷新后读取 CPA 授权列表" }}
      </p>
    </div>
    <div class="cpa-panel-note">
      <span>{{
        gateway?.updatedAt
          ? `更新于 ${accountTimestamp(gateway.updatedAt)}`
          : ""
      }}</span
      ><span v-if="gateway?.apiVersion"
        ><template v-if="gateway.gatewayVersion"
          >CPA {{ gateway.gatewayVersion }} · </template
        >{{ gateway.apiVersion }} 管理接口</span
      >
    </div>
    <a-modal
      v-model:visible="editorVisible"
      title="编辑 CPA 调度字段"
      width="480px"
      unmount-on-close
      @cancel="editorVisible = false"
    >
      <div v-if="editor" class="cpa-field-form">
        <p class="cpa-field-hint">
          这些字段由 CPA 在多个授权之间做调度时使用；留空表示沿用远端值。
        </p>
        <label>
          <span>优先级</span>
          <a-input-number
            v-model="editor.priority"
            :precision="0"
            placeholder="沿用远端"
            allow-clear
          />
        </label>
        <label>
          <span>权重</span>
          <a-input-number
            v-model="editor.weight"
            :precision="0"
            placeholder="沿用远端"
            allow-clear
          />
        </label>
        <label>
          <span>单账号重试次数</span>
          <a-input-number
            v-model="editor.requestRetry"
            :precision="0"
            :min="0"
            placeholder="沿用远端"
            allow-clear
          />
        </label>
        <label>
          <span>备注</span>
          <a-input v-model="editor.note" :max-length="200" allow-clear />
        </label>
        <label>
          <span>排除模型</span>
          <a-select
            v-model="editor.excludedModels"
            mode="tags"
            placeholder="输入模型名后回车"
            allow-create
            :options="[]"
          />
        </label>
        <p class="cpa-field-hint">重试次数填 0 表示移除该账号的独立覆盖。</p>
        <a-alert v-if="editorError" type="error">{{ editorError }}</a-alert>
      </div>
      <template #footer
        ><a-button :disabled="editorSaving" @click="editorVisible = false"
          >取消</a-button
        ><a-button type="primary" :loading="editorSaving" @click="saveFields"
          >保存到 CPA</a-button
        ></template
      >
    </a-modal>
    <CpaErrorLogDialog
      :visible="logsOpen"
      :provider="provider"
      @close="logsOpen = false"
    />
    <CpaRequestLogDialog
      :visible="requestLogsOpen"
      :provider="provider"
      @close="requestLogsOpen = false"
    />
    <CpaUsageDialog
      :visible="usageOpen"
      :provider="provider"
      @close="usageOpen = false"
    />
    <CpaPluginsDialog
      :visible="pluginsOpen"
      :provider="provider"
      :revision="revision"
      @close="closePlugins"
    />
    <CpaLoginDialog
      :visible="loginOpen"
      :provider="provider"
      @close="loginOpen = false"
      @completed="loginCompleted"
    />
    <FileEditorModal
      :visible="viewerOpen"
      :title="`CPA 授权文件 · ${viewer?.name || ''}`"
      fill
      @close="closeViewer"
    >
      <div class="cpa-file-viewer">
        <a-spin v-if="viewerLoading" tip="读取远端授权文件" />
        <template v-else-if="viewer"
          ><a-alert v-if="viewer.reason" type="info">{{
            viewer.reason
          }}</a-alert
          ><ContentEditor
            :model-value="viewer.content"
            :original-text="viewer.content"
            format="json"
            readonly
            label="CPA 授权文件原文"
        /></template>
      </div>
    </FileEditorModal>
  </div>
</template>
<style scoped>
.cpa-gateway-panel {
  --cpa-accent: rgb(var(--primary-6));
  --cpa-accent-soft: color-mix(in srgb, var(--cpa-accent) 8%, transparent);
  --cpa-accent-line: color-mix(in srgb, var(--cpa-accent) 26%, transparent);
  display: grid;
  gap: 17px;
}
.cpa-panel-toolbar {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
}
.cpa-panel-toolbar > span {
  flex: 1;
  min-width: 0;
  color: var(--color-text-3);
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.cpa-toolbar-caret {
  margin-left: 1px;
  opacity: 0.6;
}
.cpa-overview {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(132px, 1fr));
  gap: 8px;
}
.cpa-overview > div,
.cpa-overview > button {
  display: grid;
  gap: 3px;
  min-width: 0;
  padding: 11px 13px;
  border: 1px solid var(--color-border-2);
  border-radius: 10px;
  background: var(--color-fill-1);
  text-align: left;
  transition:
    border-color 0.15s ease,
    background 0.15s ease;
}
.cpa-overview > button {
  cursor: pointer;
}
.cpa-overview > button:hover {
  border-color: var(--cpa-accent-line);
  background: var(--cpa-accent-soft);
}
.cpa-overview strong {
  overflow: hidden;
  color: var(--color-text-1);
  font-size: 24px;
  font-weight: 600;
  letter-spacing: -0.02em;
  line-height: 1.15;
  font-variant-numeric: tabular-nums;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.cpa-overview small {
  margin-left: 1px;
  color: var(--color-text-3);
  font-size: 13px;
  font-weight: 400;
}
.cpa-overview span {
  overflow: hidden;
  color: var(--color-text-3);
  font-size: 11px;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.cpa-overview > button:hover span {
  color: var(--color-text-2);
}
.cpa-overview-rate {
  align-content: start;
}
.cpa-overview-meter {
  height: 4px;
  margin: 1px 0 2px;
  overflow: hidden;
  border-radius: 999px;
  background: color-mix(in srgb, var(--color-fill-3) 76%, transparent);
}
.cpa-overview-meter span {
  display: block;
  width: 100%;
  height: 100%;
  border-radius: inherit;
  background: linear-gradient(90deg, #1fb27f, #4ac9a0);
  transform-origin: left;
  transition: transform 0.32s ease;
}
.cpa-selection {
  display: flex;
  align-items: center;
  gap: 14px;
}
.cpa-selection :deep(.arco-input-wrapper) {
  flex: 1;
}
.cpa-account-list {
  display: grid;
  gap: 6px;
  max-height: 420px;
  padding-right: 2px;
  overflow: auto;
}
.cpa-account-row {
  display: grid;
  border: 1px solid var(--color-border-2);
  border-radius: 10px;
  background: var(--color-fill-1);
  transition:
    border-color 0.15s ease,
    background 0.15s ease;
}
.cpa-account-row:hover {
  border-color: var(--cpa-accent-line);
}
.cpa-account-row.expanded {
  border-color: var(--cpa-accent-line);
  background: var(--cpa-accent-soft);
}
.cpa-account-main {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 12px;
}
.cpa-account-copy {
  display: grid;
  gap: 5px;
  flex: 1;
  min-width: 0;
  padding: 0;
  border: 0;
  background: none;
  text-align: left;
  cursor: pointer;
}
.cpa-account-copy strong {
  font-weight: 500;
  font-size: 13px;
  color: var(--color-text-1);
  overflow-wrap: anywhere;
}
.cpa-account-copy span {
  font-size: 11px;
  color: var(--color-text-3);
  overflow-wrap: anywhere;
}
.cpa-account-status {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  height: 19px;
  padding: 0 8px;
  border-radius: 999px;
  background: color-mix(in srgb, currentColor 10%, transparent);
  color: rgb(var(--green-6));
  font-size: 10px;
  line-height: 1;
}
.cpa-account-status::before {
  content: "";
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: currentColor;
  box-shadow: 0 0 0 2px color-mix(in srgb, currentColor 18%, transparent);
}
.cpa-account-status.unavailable {
  color: var(--color-text-3);
}
.cpa-account-detail {
  display: grid;
  gap: 12px;
  padding: 0 12px 12px 38px;
}
.cpa-detail-stats {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 8px;
}
.cpa-detail-stats > div {
  display: grid;
  gap: 1px;
  min-width: 0;
  padding: 8px 10px;
  border: 1px solid var(--color-border-2);
  border-radius: 8px;
  background: var(--color-bg-2, var(--color-fill-1));
}
.cpa-detail-stats strong {
  overflow: hidden;
  color: var(--color-text-1);
  font-size: 14px;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.cpa-detail-stats span {
  font-size: 11px;
  color: var(--color-text-3);
}
.cpa-detail-models {
  display: flex;
  flex-wrap: wrap;
  gap: 5px;
}
.cpa-model {
  max-width: 100%;
  overflow: hidden;
  border: 1px solid var(--color-border-2);
  border-radius: 999px;
  background: var(--color-fill-1);
  color: var(--color-text-2);
  font-size: 10px;
  line-height: 16px;
  padding: 0 7px;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.cpa-model.empty {
  border-color: transparent;
  background: none;
  color: var(--color-text-3);
  padding: 0;
}
.cpa-model.excluded {
  border-color: color-mix(in srgb, rgb(var(--red-6)) 26%, transparent);
  background: color-mix(in srgb, rgb(var(--red-6)) 8%, transparent);
  color: rgb(var(--red-6));
  text-decoration: line-through;
}
.cpa-detail-note {
  margin: 0;
  font-size: 12px;
  color: var(--color-text-3);
  overflow-wrap: anywhere;
}
.cpa-detail-note.warning {
  color: rgb(var(--orange-6));
}
.cpa-detail-trend {
  display: flex;
  align-items: flex-end;
  gap: 12px;
  padding-top: 2px;
}
.cpa-detail-trend-label {
  flex-shrink: 0;
  padding-bottom: 17px;
  color: var(--color-text-3);
  font-size: 11px;
}
.cpa-trend-bars {
  display: flex;
  align-items: flex-end;
  flex: 1;
  gap: 6px;
  min-width: 0;
  padding-bottom: 0;
}
.cpa-trend-bar {
  display: grid;
  justify-items: center;
  gap: 4px;
  flex: 1;
  min-width: 0;
}
.cpa-trend-columns {
  display: flex;
  align-items: flex-end;
  justify-content: center;
  width: 100%;
  height: 34px;
  border-bottom: 1px solid var(--color-border-2);
}
.cpa-trend-columns .is-column {
  position: relative;
  display: flex;
  align-items: flex-end;
  width: 11px;
  min-height: 0;
  overflow: hidden;
  border-radius: 3px 3px 1px 1px;
  background: linear-gradient(180deg, #6bd8b4, #1fb27f);
  transition: height 0.28s ease;
}
.cpa-trend-columns .is-failed {
  display: block;
  width: 100%;
  border-radius: 0;
  background: linear-gradient(180deg, #f4a1a8, #e34d59);
}
.cpa-trend-bar em {
  max-width: 100%;
  overflow: hidden;
  color: var(--color-text-3);
  font-size: 9px;
  font-style: normal;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}
.cpa-account-cooldown {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  height: 19px;
  padding: 0 8px;
  border: 1px solid color-mix(in srgb, rgb(var(--orange-6)) 32%, transparent);
  border-radius: 999px;
  background: color-mix(in srgb, rgb(var(--orange-6)) 9%, transparent);
  color: rgb(var(--orange-6));
  font-size: 10px;
}
.cpa-account-cooldown::before {
  content: "";
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: currentColor;
}
.cpa-detail-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}
.cpa-field-form {
  display: grid;
  gap: 12px;
}
.cpa-field-form label {
  display: grid;
  gap: 6px;
  font-size: 12px;
  color: var(--color-text-2);
}
.cpa-field-hint {
  margin: 0;
  font-size: 11px;
  color: var(--color-text-3);
  line-height: 1.6;
}
.cpa-file-viewer {
  display: flex;
  flex-direction: column;
  gap: 12px;
  height: calc(100vh - 150px);
  min-height: 320px;
}
.cpa-file-viewer :deep(.content-editor) {
  flex: 1;
  min-height: 0;
}
.cpa-panel-note {
  display: flex;
  justify-content: space-between;
  color: var(--color-text-3);
  font-size: 11px;
}
.cpa-empty {
  color: var(--color-text-3);
  font-size: 12px;
  margin: 0;
  line-height: 1.65;
}
</style>
