<script setup lang="ts">
import { computed } from "vue";
import {
  IconCheck,
  IconCopy,
  IconDelete,
  IconEdit,
  IconLoading,
  IconLock,
  IconPlus,
  IconRefresh,
} from "@arco-design/web-vue/es/icon";
import type { ApiKeyManagerOperation } from "../../composables/useApiKeyManager";
import type { Provider, ProviderApiKeyOption } from "../../stores/providers";
import { useCliRuntimeStore } from "../../stores/cli-runtime";
import { agentCliLabel } from "../../utils/cli-environment";
import {
  formatQuotaValue,
  maskApiKey,
  providerApiKeyDisplayName,
  providerApiKeySecondaryName,
  providerUsesApiKeyOption,
} from "../../utils/provider-display";
import AgentCliIcon from "../AgentCliIcon.vue";

const props = defineProps<{
  createVisible: boolean;
  createName: string;
  addVisible: boolean;
  addRemark: string;
  addValue: string;
  remarkVisible: boolean;
  remarkValue: string;
  remarkTarget: ProviderApiKeyOption | null;
  provider: Provider | null;
  operation: ApiKeyManagerOperation | null;
  keys: ProviderApiKeyOption[];
  remoteManaged: boolean;
}>();

const emit = defineEmits<{
  "update:createVisible": [visible: boolean];
  "update:createName": [name: string];
  "update:addVisible": [visible: boolean];
  "update:addRemark": [remark: string];
  "update:addValue": [value: string];
  "update:remarkVisible": [visible: boolean];
  "update:remarkValue": [remark: string];
  sync: [];
  "show-create": [];
  "show-add": [];
  "show-remark": [option: ProviderApiKeyOption];
  create: [];
  "add-local": [];
  "save-remark": [];
  "set-default": [option: ProviderApiKeyOption];
  copy: [option: ProviderApiKeyOption];
  delete: [option: ProviderApiKeyOption];
}>();

const cliStore = useCliRuntimeStore();
const busy = computed(() => props.operation !== null);
const usableKeyCount = computed(() => props.keys.filter((option) => option.keyAvailable).length);
const currentOption = computed(() => props.keys.find((option) => isDefault(option)) ?? null);
const agentBindingCount = computed(() => {
  const providerId = props.provider?.identity.id;
  if (!providerId) return 0;
  return cliStore.cliRuntime.configs.filter((snapshot) => snapshot.providerId === providerId).length;
});
const operationLabel = computed(() => {
  const labels: Record<ApiKeyManagerOperation, string> = {
    sync: "正在同步站点 Key",
    create: "正在创建站点 Key",
    add: "正在保存 API Key",
    remark: "正在保存备注",
    default: "正在切换当前调用 Key",
    delete: "正在删除 API Key",
  };
  return props.operation ? labels[props.operation] : "";
});
const currentCallDescription = computed(() =>
  currentOption.value
    ? "卡片刷新、模型请求和未单独指定 Key 的临时 CLI 会使用这一把。"
    : "请先从下方选择一把完整 Key，卡片才能确定默认请求凭据。",
);
const createNameModel = computed({
  get: () => props.createName,
  set: (value: string) => emit("update:createName", value),
});
const addRemarkModel = computed({
  get: () => props.addRemark,
  set: (value: string) => emit("update:addRemark", value),
});
const addValueModel = computed({
  get: () => props.addValue,
  set: (value: string) => emit("update:addValue", value),
});
const remarkValueModel = computed({
  get: () => props.remarkValue,
  set: (value: string) => emit("update:remarkValue", value),
});

function displayMaskedKey(option: ProviderApiKeyOption) {
  return option.maskedKey?.trim() || maskApiKey(option.key) || "完整 Key 不可读取";
}

function isDefault(option: ProviderApiKeyOption) {
  const provider = props.provider;
  return provider ? providerUsesApiKeyOption(provider, option) : false;
}

function isRemoteKey(option: ProviderApiKeyOption) {
  return Boolean(option.tokenId.trim());
}

function canDelete(option: ProviderApiKeyOption) {
  return Boolean(option.localId.trim());
}

function deleteLabel(option: ProviderApiKeyOption) {
  if (isRemoteKey(option)) {
    return props.remoteManaged
      ? "从站点撤销这把 Key"
      : "仅从当前卡片移除，不会撤销站点令牌";
  }
  return option.localId.trim() ? "从当前卡片移除这把 Key" : "当前 Key 不可移除";
}

function agentBindings(option: ProviderApiKeyOption) {
  const provider = props.provider;
  if (!provider) return [];
  return cliStore.cliRuntime.configs.filter((snapshot) => {
    if (snapshot.providerId !== provider.identity.id) return false;
    const localId = snapshot.apiKeyLocalId?.trim() || "";
    return localId ? localId === option.localId.trim() : isDefault(option);
  });
}

function agentBindingTitle(option: ProviderApiKeyOption) {
  const labels = agentBindings(option).map((snapshot) =>
    agentCliLabel(cliStore.cliEnvironmentProbe, snapshot.cliKind),
  );
  return labels.length > 0 ? `Agent 默认配置：${labels.join("、")}` : "";
}

function statusLabel(status: string) {
  const value = String(status || "").trim().toLowerCase();
  if (value === "1" || value === "enabled") return "启用";
  if (value === "2" || value === "disabled") return "停用";
  if (value === "3" || value === "expired") return "过期";
  if (value === "4" || value === "exhausted") return "耗尽";
  return value || "已同步";
}

function displayStatus(option: ProviderApiKeyOption) {
  if (!option.keyAvailable) return "不可读取";
  return isRemoteKey(option) ? statusLabel(option.status) : "已保存";
}

function statusTone(option: ProviderApiKeyOption) {
  if (!option.keyAvailable) return "unknown";
  if (!isRemoteKey(option)) return "saved";
  const value = String(option.status || "").trim().toLowerCase();
  if (value === "1" || value === "enabled") return "enabled";
  if (value === "2" || value === "disabled") return "disabled";
  if (value === "3" || value === "expired") return "expired";
  if (value === "4" || value === "exhausted") return "exhausted";
  return "unknown";
}

function quotaText(option: ProviderApiKeyOption) {
  if (option.unlimitedQuota) return "无限额度";
  if (!option.usedQuotaRaw && !option.remainQuotaRaw) return "额度未公开";
  return `剩余 ${formatQuotaValue(option.remainQuota || 0, {
    quotaDisplayType: option.quotaDisplayType || "currency",
    currencySymbol: option.currencySymbol || "$",
  })}`;
}

function keyIdentity(option: ProviderApiKeyOption) {
  const parts = [];
  if (option.tokenId) parts.push(`站点 ID ${option.tokenId}`);
  if (option.userId) parts.push(`用户 ${option.userId}`);
  return parts.join(" · ");
}

function closeCreate() {
  emit("update:createVisible", false);
}

function closeAdd() {
  emit("update:addVisible", false);
}

function closeRemark() {
  emit("update:remarkVisible", false);
}
</script>

<template>
  <section v-if="provider" class="provider-form-block api-key-manager" aria-label="API Key 管理">
    <header class="provider-form-block-header api-key-manager-header">
      <span class="provider-form-block-icon provider-form-block-icon-auth"><IconLock /></span>
      <div>
        <strong>API Key</strong>
        <small>这里的新增、备注、切换和删除会立即保存</small>
      </div>
      <span class="provider-form-block-meta">{{ keys.length }} 把</span>
    </header>

    <div class="api-key-manager-body">
      <section v-if="keys.length > 0" class="api-key-manager-commandbar">
        <div class="api-key-manager-scope">
          <strong>{{ remoteManaged ? "站点与本地 Key" : "本地 Key" }}</strong>
          <span>
            {{ usableKeyCount }} 把可直接使用
            <template v-if="agentBindingCount > 0"> · {{ agentBindingCount }} 个 Agent 绑定</template>
          </span>
        </div>
        <div class="api-key-manager-toolbar">
          <a-button
            v-if="remoteManaged"
            :loading="operation === 'sync'"
            :disabled="busy"
            @click="emit('sync')"
          >
            <template #icon><IconRefresh /></template>
            同步站点 Key
          </a-button>
          <a-button :type="remoteManaged ? 'secondary' : 'primary'" :disabled="busy" @click="emit('show-add')">
            <template #icon><IconPlus /></template>
            {{ remoteManaged ? "添加已有 Key" : "添加 API Key" }}
          </a-button>
          <a-button v-if="remoteManaged" type="primary" :disabled="busy" @click="emit('show-create')">
            <template #icon><IconPlus /></template>
            创建站点 Key
          </a-button>
        </div>
      </section>

      <div v-if="operationLabel" class="api-key-manager-operation" role="status" aria-live="polite">
        <IconLoading />
        <span>{{ operationLabel }}</span>
      </div>

      <section v-if="addVisible" class="api-key-inline-editor">
        <header>
          <div>
            <strong>添加已有 API Key</strong>
            <small>保存到当前卡片，不会在站点创建新令牌</small>
          </div>
          <button type="button" aria-label="收起添加表单" @click="closeAdd">收起</button>
        </header>
        <div class="api-key-inline-fields">
          <label>
            <span>完整 API Key</span>
            <a-input-password v-model="addValueModel" allow-clear placeholder="粘贴完整 API Key" />
          </label>
          <label>
            <span>备注（可选）</span>
            <a-input v-model="addRemarkModel" allow-clear placeholder="例如：Codex 主用、备用 Key" @press-enter="emit('add-local')" />
          </label>
        </div>
        <footer>
          <a-button @click="closeAdd">取消</a-button>
          <a-button type="primary" :loading="operation === 'add'" :disabled="!addValueModel.trim()" @click="emit('add-local')">
            保存 API Key
          </a-button>
        </footer>
      </section>

      <section v-if="createVisible && remoteManaged" class="api-key-inline-editor">
        <header>
          <div>
            <strong>创建站点 API Key</strong>
            <small>创建成功后会同步保存到当前卡片</small>
          </div>
          <button type="button" aria-label="收起创建表单" @click="closeCreate">收起</button>
        </header>
        <div class="api-key-inline-fields api-key-inline-fields-single">
          <label>
            <span>站点 Key 名称</span>
            <a-input v-model="createNameModel" allow-clear placeholder="例如：Claude Code、备用密钥" @press-enter="emit('create')" />
          </label>
        </div>
        <footer>
          <a-button @click="closeCreate">取消</a-button>
          <a-button type="primary" :loading="operation === 'create'" :disabled="!createNameModel.trim()" @click="emit('create')">
            创建 Key
          </a-button>
        </footer>
      </section>

      <section v-if="remarkVisible && remarkTarget" class="api-key-inline-editor">
        <header>
          <div>
            <strong>修改备注</strong>
            <small>仅保存在本机，不会修改站点上的 Key 名称</small>
          </div>
          <button type="button" aria-label="收起备注表单" @click="closeRemark">收起</button>
        </header>
        <div class="api-key-inline-fields api-key-inline-fields-single">
          <label>
            <span>{{ providerApiKeyDisplayName(remarkTarget) }}</span>
            <a-input v-model="remarkValueModel" allow-clear placeholder="留空保存即可清除备注" @press-enter="emit('save-remark')" />
          </label>
        </div>
        <footer>
          <a-button @click="closeRemark">取消</a-button>
          <a-button type="primary" :loading="operation === 'remark'" @click="emit('save-remark')">保存备注</a-button>
        </footer>
      </section>

      <section
        v-if="keys.length > 0"
        class="api-key-current-call"
        :class="{ warning: !currentOption }"
        aria-label="当前调用 API Key"
      >
        <span class="api-key-current-call-icon"><IconCheck v-if="currentOption" /><IconLock v-else /></span>
        <div class="api-key-current-call-copy">
          <small>当前调用 Key</small>
          <strong>{{ currentOption ? providerApiKeyDisplayName(currentOption) : "尚未选择" }}</strong>
          <code v-if="currentOption">{{ displayMaskedKey(currentOption) }}</code>
          <span>{{ currentCallDescription }}</span>
        </div>
        <div v-if="currentOption && agentBindings(currentOption).length > 0" class="api-key-current-call-agents" :title="agentBindingTitle(currentOption)">
          <small>Agent 独立绑定</small>
          <span>
            <AgentCliIcon
              v-for="binding in agentBindings(currentOption)"
              :key="binding.cliKind"
              :kind="binding.cliKind"
              :size="16"
            />
          </span>
        </div>
      </section>

      <div v-if="keys.length === 0" class="api-key-empty">
        <span class="api-key-empty-icon"><IconLock /></span>
        <strong>{{ remoteManaged ? "当前卡片还没有 API Key" : "还没有保存 API Key" }}</strong>
        <span>{{ remoteManaged ? "可以同步站点已有 Key，也可以添加或创建一把 Key。" : "添加第一把 Key 后，卡片刷新、模型请求和临时 CLI 才能使用它。" }}</span>
        <div>
          <a-button
            v-if="remoteManaged"
            type="primary"
            :loading="operation === 'sync'"
            :disabled="busy"
            @click="emit('sync')"
          >
            同步站点 Key
          </a-button>
          <a-button :type="remoteManaged ? 'secondary' : 'primary'" :disabled="busy" @click="emit('show-add')">
            {{ remoteManaged ? "添加已有 Key" : "添加 API Key" }}
          </a-button>
          <a-button v-if="remoteManaged" :disabled="busy" @click="emit('show-create')">创建站点 Key</a-button>
        </div>
      </div>

      <section v-else class="api-key-vault">
        <header class="api-key-vault-heading">
          <strong>全部 Key</strong>
          <span>Agent CLI 可以独立绑定任意一把，不必跟随当前调用 Key</span>
        </header>
        <div class="api-key-vault-list" role="list" aria-label="全部 API Key">
          <article
            v-for="option in keys"
            :key="option.localId || option.tokenId || option.key"
            class="api-key-vault-item"
            :class="{ default: isDefault(option) }"
            role="listitem"
          >
            <span class="api-key-vault-marker" :class="{ active: isDefault(option) }">
              <IconCheck v-if="isDefault(option)" />
              <IconLock v-else />
            </span>
            <div class="api-key-vault-main">
              <div class="api-key-vault-title-row">
                <strong>{{ providerApiKeyDisplayName(option) }}</strong>
                <span v-if="isDefault(option)" class="api-key-default-badge">当前调用</span>
                <span class="api-key-source-badge">{{ isRemoteKey(option) ? "站点" : "本机" }}</span>
                <span class="api-key-status" :class="`api-key-status-${statusTone(option)}`">{{ displayStatus(option) }}</span>
              </div>
              <small v-if="providerApiKeySecondaryName(option)" class="api-key-vault-remote-name">
                站点名称：{{ providerApiKeySecondaryName(option) }}
              </small>
              <code>{{ displayMaskedKey(option) }}</code>
              <span
                v-if="agentBindings(option).length > 0"
                class="api-key-agent-bindings"
                :title="agentBindingTitle(option)"
              >
                <small>Agent 使用</small>
                <AgentCliIcon
                  v-for="binding in agentBindings(option)"
                  :key="binding.cliKind"
                  :kind="binding.cliKind"
                  :size="15"
                />
              </span>
            </div>
            <div v-if="remoteManaged && isRemoteKey(option)" class="api-key-vault-summary">
              <span>{{ quotaText(option) }}</span>
              <span v-if="option.group">分组 {{ option.group }}</span>
              <span v-if="option.modelLimitsEnabled">模型 {{ option.modelLimits.length }}</span>
              <span v-if="option.allowIps.length">IP {{ option.allowIps.length }}</span>
              <small v-if="keyIdentity(option)">{{ keyIdentity(option) }}</small>
            </div>
            <div class="api-key-actions">
              <a-button
                v-if="!isDefault(option)"
                size="small"
                type="text"
                class="api-key-set-default"
                :disabled="busy || !option.keyAvailable || !option.localId"
                @click="emit('set-default', option)"
              >
                <template #icon><IconCheck /></template>
                设为当前
              </a-button>
              <a-tooltip content="复制 API Key">
                <a-button size="small" type="text" :disabled="!option.keyAvailable" aria-label="复制 API Key" @click="emit('copy', option)">
                  <template #icon><IconCopy /></template>
                </a-button>
              </a-tooltip>
              <a-tooltip content="修改备注">
                <a-button size="small" type="text" :disabled="busy || !option.localId" aria-label="修改 API Key 备注" @click="emit('show-remark', option)">
                  <template #icon><IconEdit /></template>
                </a-button>
              </a-tooltip>
              <a-tooltip :content="deleteLabel(option)">
                <a-button
                  size="small"
                  type="text"
                  status="danger"
                  :disabled="busy || !canDelete(option)"
                  aria-label="删除或移除 API Key"
                  @click="emit('delete', option)"
                >
                  <template #icon><IconDelete /></template>
                </a-button>
              </a-tooltip>
            </div>
          </article>
        </div>
      </section>
    </div>
  </section>
</template>
