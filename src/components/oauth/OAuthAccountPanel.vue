<script setup lang="ts">
import { computed, onScopeDispose, ref, watch } from "vue";
import { KeyRound, RefreshCw, Download, FileCode2 } from "@lucide/vue";
import { Message } from "@arco-design/web-vue";
import AgentCliIcon from "../AgentCliIcon.vue";
import {
  getOAuthAccount,
  setOAuthRenewal,
  updateOAuthAccount,
} from "../../api/oauth-accounts";
import type { Provider } from "../../stores/provider-types";
import type { OAuthAccountDetails } from "../../stores/oauth-types";
import { quotaLevel } from "../../utils/quota-level";
import { useProviderStore } from "../../stores/providers";
import {
  accountTimestamp,
  firstQuotaExhaustion,
  managedModelCount,
  nextQuotaReset,
  oauthVendorLabel,
  planLabel,
  quotaAlertWindows,
  quotaNumber,
  quotaPercent,
  quotaResetLabel,
  quotaWindowPercent,
  splitQuotaWindows,
  tokenExpiryLabel,
  tokenExpirySoon,
} from "../../utils/oauth-display";
import { confirmAction } from "../../composables/provider-credential-dialogs";
import AccountQuotaBar from "./AccountQuotaBar.vue";
import OAuthExportPanel from "./OAuthExportPanel.vue";
import OAuthCredentialEditor from "./OAuthCredentialEditor.vue";
const props = defineProps<{ provider: Provider }>();
const emit = defineEmits<{ authorize: []; models: [] }>();
const store = useProviderStore();
const details = ref<OAuthAccountDetails | null>(null);
const loading = ref(false);
const busy = ref(false);
const error = ref("");
const section = ref<"usage" | "export">("usage");
const credentialEditor = ref(false);
const name = ref(props.provider.identity.name);
const remark = ref(props.provider.identity.remark);
const interval = ref(props.provider.automation.refreshInterval);
let request = 0;
let disposed = false;
const oauth = computed(() => props.provider.auth.oauth!);
const subscription = computed(() => props.provider.capabilities.subscription);
const windows = computed(() => subscription.value?.windows || []);
/// Model-scoped windows answer a different question than the account-wide
/// ones, so the detail view separates them instead of interleaving both.
const windowGroups = computed(() => splitQuotaWindows(windows.value));
const alerting = computed(() => quotaAlertWindows(windows.value));
/// The tightest window is the one that decides whether the account can still
/// work; Rust already orders them that way, so the panel just names the head.
const tightest = computed(() => windows.value[0] || null);
const tightestPercent = computed(() =>
  tightest.value ? quotaWindowPercent(tightest.value) : null,
);
const tightestLevel = computed(() => quotaLevel(tightestPercent.value));
const nextReset = computed(() => nextQuotaReset(windows.value));
/// Predicted exhaustion is an estimate from our own samples, so it is offered
/// beside the measured windows rather than replacing any of their readings.
const projectedExhaustion = computed(() =>
  firstQuotaExhaustion(windows.value),
);
const tokenExpiry = computed(() =>
  tokenExpiryLabel(props.provider.auth.accessTokenExpiresAt),
);
const tokenExpiring = computed(() =>
  tokenExpirySoon(props.provider.auth.accessTokenExpiresAt),
);
const modelCount = computed(() => managedModelCount(props.provider));
const renewal = computed(() =>
  oauth.value.refreshPending
    ? "上次续期结果待确认"
    : oauth.value.refreshError
      ? "续期已暂停"
      : details.value?.renewalEnabledHere
        ? "本机自动续期"
        : details.value?.renewalElsewhere
          ? "其他设备管理续期"
          : "本机续期已关闭",
);
onScopeDispose(() => {
  disposed = true;
  request++;
});
async function load() {
  const sequence = ++request;
  const id = props.provider.identity.id;
  loading.value = true;
  try {
    const result = await getOAuthAccount(id);
    if (
      !disposed &&
      sequence === request &&
      props.provider.identity.id === id
    ) {
      details.value = result;
      error.value = "";
    }
  } catch (cause) {
    if (!disposed && sequence === request) error.value = String(cause);
  } finally {
    if (sequence === request) loading.value = false;
  }
}
watch(
  () => [props.provider.identity.id, props.provider.auth.credentialRevision],
  () => {
    void load();
  },
  { immediate: true },
);
async function refresh() {
  if (busy.value) return;
  busy.value = true;
  try {
    const failure = await store.refreshByIds([props.provider.identity.id]);
    if (failure && !disposed) error.value = failure;
  } catch (cause) {
    if (!disposed) error.value = String(cause);
  } finally {
    busy.value = false;
  }
}
async function renewalChanged(enabled: boolean | string | number) {
  if (!details.value || busy.value) return;
  const id = props.provider.identity.id;
  const revision = props.provider.auth.credentialRevision;
  busy.value = true;
  try {
    if (
      enabled &&
      !(await confirmAction(
        "由本机续期",
        "请确认其他应用或设备已暂停此账号的续期。多个应用同时刷新同一授权，可能使授权失效。",
        "由本机接管",
        "warning",
      ))
    )
      return;
    if (
      disposed ||
      props.provider.identity.id !== id ||
      props.provider.auth.credentialRevision !== revision
    )
      return;
    store.upsertProvider(await setOAuthRenewal(id, Boolean(enabled)));
    if (!disposed) await load();
  } catch (cause) {
    if (!disposed) error.value = String(cause);
  } finally {
    busy.value = false;
  }
}
function credentialSaved(provider: Provider) {
  store.upsertProvider(provider);
  void load();
}
async function saveMetadata() {
  if (busy.value) return;
  busy.value = true;
  try {
    store.upsertProvider(
      await updateOAuthAccount(
        props.provider.identity.id,
        name.value,
        remark.value,
        interval.value,
      ),
    );
    if (!disposed) Message.success("账号设置已保存");
  } catch (cause) {
    if (!disposed) error.value = String(cause);
  } finally {
    busy.value = false;
  }
}
</script>
<template>
  <div class="oauth-account-panel">
    <div class="oauth-account-toolbar">
      <div class="oauth-account-tabs" role="tablist" aria-label="账号管理">
        <button
          type="button"
          role="tab"
          :aria-selected="section === 'usage'"
          :class="{ active: section === 'usage' }"
          @click="section = 'usage'"
        >
          额度与授权</button
        ><button
          type="button"
          role="tab"
          :aria-selected="section === 'export'"
          :class="{ active: section === 'export' }"
          @click="section = 'export'"
        >
          <Download :size="13" />导出与交接
        </button>
      </div>
      <div class="oauth-account-toolbar-actions">
        <a-button size="small" @click="credentialEditor = true"
          ><template #icon><FileCode2 :size="13" /></template>授权原文</a-button
        ><a-button size="small" :loading="busy" @click="refresh"
          ><template #icon><RefreshCw :size="13" /></template>刷新</a-button
        >
      </div>
    </div>
    <a-alert v-if="error" type="error"
      >{{ error
      }}<a-button v-if="!details" size="mini" @click="load"
        >重试</a-button
      ></a-alert
    >
    <template v-if="section === 'usage'">
      <div class="oauth-account-identity">
        <div class="oauth-account-identity-main">
          <AgentCliIcon :kind="oauth.vendor" :size="28" />
          <div>
            <strong>{{ oauth.email || oauthVendorLabel(oauth.vendor) }}</strong
            ><span
              >{{ planLabel(subscription?.plan || oauth.plan) ||
              `${oauthVendorLabel(oauth.vendor)} 订阅`
              }}<template
                v-if="subscription?.organization || oauth.organizationName"
              >
                ·
                {{
                  subscription?.organization || oauth.organizationName
                }}</template
              ></span
            >
          </div>
        </div>
        <a-button size="small" @click="emit('authorize')"
          ><template #icon><KeyRound :size="13" /></template>重新授权</a-button
        >
      </div>
      <a-alert v-if="subscription?.error" type="warning"
        >{{ subscription.error
        }}<template v-if="subscription.updatedAt">
          · 数据时间 {{ accountTimestamp(subscription.updatedAt) }}</template
        ></a-alert
      >
      <div class="oauth-account-summary">
        <span
          v-if="tightest && tightestPercent != null"
          class="oauth-account-chip"
          :class="`is-${tightestLevel}`"
          :title="`${tightest.label} 已用 ${quotaPercent(tightestPercent)}%`"
          >最紧张 {{ tightest.label }} {{ quotaPercent(tightestPercent) }}%</span
        ><span
          v-if="projectedExhaustion"
          class="oauth-account-chip is-high"
          title="根据本机记录的用速估算，实际以服务端额度为准"
          >{{ projectedExhaustion.window.label }}
          {{ projectedExhaustion.label }}</span
        ><span
          v-if="alerting.length"
          class="oauth-account-chip is-high"
          :title="alerting.map((window) => window.label).join(' · ')"
          >{{ alerting.length }} 项额度告急</span
        ><span
          v-if="nextReset"
          class="oauth-account-chip"
          :title="`${nextReset.label} 的重置时间`"
          >最近重置 {{ nextReset.label }}
          {{ quotaResetLabel(nextReset.resetAt) }}</span
        ><span v-if="subscription?.creditsUnlimited" class="oauth-account-chip"
          >余额不限额</span
        ><span
          v-else-if="subscription?.credits != null"
          class="oauth-account-chip"
          >余额 {{ quotaNumber(subscription.credits) }}
          {{ subscription.creditsUnit || "" }}</span
        ><span
          v-if="subscription?.updatedAt"
          class="oauth-account-chip"
          :title="`额度同步时间 ${accountTimestamp(subscription.updatedAt)}`"
          >更新于 {{ accountTimestamp(subscription.updatedAt) }}</span
        ><a-button size="mini" type="text" @click="emit('models')">{{
          modelCount == null ? "查看可用模型" : `${modelCount} 个可用模型`
        }}</a-button>
      </div>
      <template v-if="windows.length">
        <section
          v-if="windowGroups.shared.length"
          class="oauth-account-window-group"
        >
          <h4 v-if="windowGroups.scoped.length">通用额度</h4>
          <div class="oauth-account-windows">
            <AccountQuotaBar
              v-for="window in windowGroups.shared"
              :key="window.id"
              :window="window"
            />
          </div>
        </section>
        <section
          v-if="windowGroups.scoped.length"
          class="oauth-account-window-group"
        >
          <h4>
            模型专属额度
            <small>{{ windowGroups.scoped.length }}</small>
          </h4>
          <div class="oauth-account-windows">
            <AccountQuotaBar
              v-for="window in windowGroups.scoped"
              :key="window.id"
              :window="window"
            />
          </div>
        </section>
      </template>
      <p v-else class="oauth-muted">
        {{ provider.runtime.errorMessage || "尚无额度数据，刷新后查看。" }}
      </p>
      <div class="oauth-renewal">
        <div>
          <strong>{{ renewal }}</strong
          ><span
            :class="{ 'is-warning': tokenExpiring }"
            :title="`到期时间 ${accountTimestamp(
              provider.auth.accessTokenExpiresAt,
            )}`"
            >访问令牌{{
              tokenExpiry ? ` ${tokenExpiry}` : "到期时间未提供"
            }}</span
          >
        </div>
        <a-switch
          :model-value="details?.renewalEnabledHere || false"
          :loading="busy || loading"
          :disabled="
            !details ||
            (!details.renewalEnabledHere &&
              (!details.refreshable ||
                oauth.refreshPending ||
                !!oauth.refreshError))
          "
          aria-label="本机自动续期"
          @change="renewalChanged"
        />
      </div>
      <p v-if="oauth.refreshPending || oauth.refreshError" class="oauth-muted">
        {{
          oauth.refreshError ||
          "上次续期在确认结果前中断，请重新授权，避免重复使用可能已经轮换的令牌。"
        }}
      </p>
      <details class="oauth-account-settings">
        <summary>账号信息与设置</summary>
        <dl>
          <div>
            <dt>授权来源</dt>
            <dd>{{ oauth.source || "未记录" }}</dd>
          </div>
          <div v-if="oauth.accountId">
            <dt>账号 / 工作区</dt>
            <dd>{{ oauth.accountId }}</dd>
          </div>
          <div v-if="oauth.organizationId">
            <dt>组织</dt>
            <dd>{{ oauth.organizationId }}</dd>
          </div>
          <div v-if="oauth.projectId">
            <dt>Google 项目</dt>
            <dd>{{ oauth.projectId }}</dd>
          </div>
          <div>
            <dt>授权范围</dt>
            <dd>{{ oauth.scopes.join(" · ") || "未提供" }}</dd>
          </div>
        </dl>
        <a-form :model="{ name, remark, interval }" layout="vertical"
          ><a-form-item label="卡片名称"
            ><a-input v-model="name" :max-length="100" /></a-form-item
          ><a-form-item label="备注"
            ><a-input v-model="remark" :max-length="160" /></a-form-item
          ><a-form-item label="自动刷新间隔"
            ><a-select
              v-model="interval"
              :options="[
                { value: 0, label: '使用全局设置' },
                { value: 300, label: '5 分钟' },
                { value: 900, label: '15 分钟' },
                { value: 1800, label: '30 分钟' },
                { value: 3600, label: '1 小时' },
              ]" /></a-form-item
          ><a-button :loading="busy" @click="saveMetadata"
            >保存设置</a-button
          ></a-form
        >
      </details>
    </template>
    <OAuthExportPanel
      v-else-if="details"
      :account="details"
      @changed="load"
    /><a-spin v-else-if="loading" tip="读取授权信息" />
    <OAuthCredentialEditor
      :provider="provider"
      :visible="credentialEditor"
      @close="credentialEditor = false"
      @saved="credentialSaved"
    />
  </div>
</template>
<style scoped>
.oauth-account-panel {
  display: grid;
  gap: 17px;
}
.oauth-account-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.oauth-account-toolbar-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}
.oauth-account-tabs {
  display: flex;
  align-items: center;
  gap: 16px;
  border-bottom: 1px solid var(--color-border-2);
}
.oauth-account-tabs button {
  display: flex;
  align-items: center;
  gap: 5px;
  padding: 9px 0;
  border: 0;
  border-bottom: 2px solid transparent;
  background: none;
  font-size: 13px;
  color: var(--color-text-3);
  cursor: pointer;
}
.oauth-account-tabs button.active {
  color: var(--color-text-1);
  border-bottom-color: rgb(var(--primary-6));
}
.oauth-account-identity {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
}
.oauth-account-identity-main {
  display: flex;
  align-items: center;
  gap: 11px;
  min-width: 0;
}
.oauth-account-identity-main > div {
  display: grid;
  gap: 6px;
  min-width: 0;
}
.oauth-account-identity strong {
  font-size: 16px;
  font-weight: 550;
  color: var(--color-text-1);
  overflow-wrap: anywhere;
}
.oauth-account-identity span {
  font-size: 12px;
  color: var(--color-text-3);
}
.oauth-account-window-group {
  display: grid;
  gap: 10px;
}
.oauth-account-window-group h4 {
  display: flex;
  align-items: baseline;
  gap: 6px;
  margin: 0;
  color: var(--color-text-2);
  font-size: 12px;
  font-weight: 500;
}
.oauth-account-window-group h4 small {
  color: var(--color-text-3);
  font-size: 10px;
  font-weight: 400;
}
.oauth-account-windows {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 18px 28px;
  padding: 18px;
  background: var(--color-fill-1);
  border: 1px solid var(--color-border-2);
  border-radius: 10px;
}
.oauth-account-summary {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
  font-size: 11px;
}
.oauth-account-chip {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  overflow: hidden;
  border: 1px solid var(--color-border-2);
  border-radius: 999px;
  background: var(--color-fill-1);
  color: var(--color-text-3);
  line-height: 16px;
  padding: 1px 8px;
  font-variant-numeric: tabular-nums;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.oauth-account-chip.is-warn {
  border-color: rgba(217, 138, 0, 0.34);
  background: rgba(217, 138, 0, 0.09);
  color: #b26a00;
}
.oauth-account-chip.is-high {
  border-color: rgba(227, 77, 89, 0.3);
  background: rgba(227, 77, 89, 0.09);
  color: #cb2634;
}
.oauth-account-summary :deep(.arco-btn) {
  margin-left: auto;
}
.oauth-renewal {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding-top: 16px;
  border-top: 1px solid var(--color-border-2);
}
.oauth-renewal > div {
  display: grid;
  gap: 6px;
}
.oauth-renewal strong {
  font-size: 13px;
  font-weight: 500;
  color: var(--color-text-1);
}
.oauth-renewal span {
  color: var(--color-text-3);
  font-size: 12px;
}
.oauth-renewal span.is-warning {
  color: #b26a00;
}
.oauth-muted {
  margin: 0;
  color: var(--color-text-3);
  line-height: 1.7;
  font-size: 12px;
}
.oauth-account-settings summary {
  cursor: pointer;
  color: var(--color-text-2);
  font-size: 12px;
}
.oauth-account-settings dl {
  font-size: 12px;
  margin: 15px 0;
  display: grid;
  gap: 10px;
}
.oauth-account-settings dl > div {
  display: grid;
  grid-template-columns: 94px 1fr;
  gap: 12px;
}
.oauth-account-settings dt {
  color: var(--color-text-3);
}
.oauth-account-settings dd {
  margin: 0;
  color: var(--color-text-2);
  overflow-wrap: anywhere;
}
.oauth-account-settings :deep(.arco-form) {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 12px;
}
.oauth-account-settings :deep(.arco-form-item) {
  margin-bottom: 0;
}
.oauth-account-settings :deep(.arco-btn) {
  align-self: end;
  justify-self: end;
}
@media (max-width: 580px) {
  .oauth-account-windows {
    grid-template-columns: 1fr;
  }
}
</style>
