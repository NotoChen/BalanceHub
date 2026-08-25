import { computed, ref, watch, type Ref } from "vue";
import { Message, Modal } from "@arco-design/web-vue";
import type { Provider, ProviderApiKeyOption } from "../stores/providers";
import {
  providerApiKeyDisplayName,
  providerUsesApiKeyOption,
} from "../utils/provider-display";
import { effectiveProviderApiKeyOptions } from "../utils/provider-api-key-options";
import { copyText } from "./useClipboard";

export type ApiKeyManagerOperation =
  | "sync"
  | "create"
  | "add"
  | "remark"
  | "default"
  | "delete";

interface UseApiKeyManagerOptions {
  providers: Ref<Provider[]>;
  syncRemoteKeys: (providerId: string) => Promise<ProviderApiKeyOption[]>;
  addLocalKey: (providerId: string, key: string, remark: string) => Promise<Provider>;
  createRemoteKey: (providerId: string, name: string) => Promise<ProviderApiKeyOption[]>;
  setRemark: (providerId: string, localId: string, remark: string) => Promise<Provider>;
  setDefaultKey: (providerId: string, localId: string) => Promise<Provider>;
  removeLocalKey: (providerId: string, localId: string) => Promise<Provider>;
  deleteRemoteKey: (providerId: string, tokenId: string) => Promise<ProviderApiKeyOption[]>;
  getProvider: (providerId: string) => Provider | undefined;
  getBoundAgentLabels?: (providerId: string, localId: string, current: boolean) => string[];
  onProviderUpdated?: (provider: Provider) => void;
}

export function useApiKeyManager(options: UseApiKeyManagerOptions) {
  const apiKeyManagerProvider = ref<Provider | null>(null);
  const apiKeyManagerOperation = ref<ApiKeyManagerOperation | null>(null);
  const apiKeyManagerKeys = ref<ProviderApiKeyOption[]>([]);
  const apiKeyCreateVisible = ref(false);
  const apiKeyCreateName = ref("");
  const apiKeyAddVisible = ref(false);
  const apiKeyAddRemark = ref("");
  const apiKeyAddValue = ref("");
  const apiKeyRemarkVisible = ref(false);
  const apiKeyRemarkValue = ref("");
  const apiKeyRemarkTarget = ref<ProviderApiKeyOption | null>(null);
  let requestRevision = 0;

  const apiKeyRemoteManaged = computed(() =>
    Boolean(apiKeyManagerProvider.value?.actions.apiKeyManagement),
  );

  function currentRequest(providerId: string, revision: number) {
    return revision === requestRevision
      && apiKeyManagerProvider.value?.identity.id === providerId
      && apiKeyManagerProvider.value !== null;
  }

  function applyProvider(provider: Provider | null | undefined) {
    if (!provider) return;
    apiKeyManagerProvider.value = provider;
    apiKeyManagerKeys.value = effectiveProviderApiKeyOptions(
      provider.auth.apiKey,
      provider.auth.apiKeyOptions || [],
    );
    options.onProviderUpdated?.(provider);
  }

  function currentProvider(providerId: string) {
    return options.getProvider(providerId) ?? apiKeyManagerProvider.value;
  }

  function resetInlineEditors() {
    apiKeyCreateVisible.value = false;
    apiKeyAddVisible.value = false;
    apiKeyRemarkVisible.value = false;
    apiKeyCreateName.value = "";
    apiKeyAddRemark.value = "";
    apiKeyAddValue.value = "";
    apiKeyRemarkValue.value = "";
    apiKeyRemarkTarget.value = null;
  }

  function activateApiKeyManager(provider: Provider) {
    requestRevision += 1;
    apiKeyManagerOperation.value = null;
    apiKeyManagerProvider.value = provider;
    apiKeyManagerKeys.value = effectiveProviderApiKeyOptions(
      provider.auth.apiKey,
      provider.auth.apiKeyOptions || [],
    );
    resetInlineEditors();
  }

  /**
   * Bind the key vault to the provider currently being edited.  The vault is
   * rendered inline in the credentials step; it is not a second modal surface.
   */
  function bindApiKeyManager(provider: Provider) {
    activateApiKeyManager(provider);
  }

  function closeApiKeyManager() {
    requestRevision += 1;
    apiKeyManagerOperation.value = null;
    apiKeyManagerProvider.value = null;
    apiKeyManagerKeys.value = [];
    resetInlineEditors();
  }

  function openApiKeyCreatePanel() {
    apiKeyAddVisible.value = false;
    apiKeyRemarkVisible.value = false;
    apiKeyCreateName.value = "";
    apiKeyCreateVisible.value = true;
  }

  function openApiKeyAddPanel() {
    apiKeyCreateVisible.value = false;
    apiKeyRemarkVisible.value = false;
    apiKeyAddRemark.value = "";
    apiKeyAddValue.value = "";
    apiKeyAddVisible.value = true;
  }

  function openApiKeyRemarkEditor(option: ProviderApiKeyOption) {
    apiKeyCreateVisible.value = false;
    apiKeyAddVisible.value = false;
    apiKeyRemarkTarget.value = option;
    apiKeyRemarkValue.value = option.localName?.trim() || "";
    apiKeyRemarkVisible.value = true;
  }

  async function syncRemoteApiKeys() {
    const provider = apiKeyManagerProvider.value;
    if (!provider || !apiKeyRemoteManaged.value) return;
    const providerId = provider.identity.id;
    const revision = ++requestRevision;
    apiKeyManagerOperation.value = "sync";
    try {
      const remoteKeys = await options.syncRemoteKeys(providerId);
      if (!currentRequest(providerId, revision)) return;
      apiKeyManagerKeys.value = remoteKeys;
      applyProvider(currentProvider(providerId));
      Message.success("站点密钥已同步");
    } catch (error) {
      if (currentRequest(providerId, revision)) {
        Message.warning(`站点密钥同步失败，本地密钥仍可使用：${errorMessage(error)}`);
      }
    } finally {
      if (currentRequest(providerId, revision)) apiKeyManagerOperation.value = null;
    }
  }

  async function runProviderMutation(
    operation: ApiKeyManagerOperation,
    action: (providerId: string) => Promise<Provider>,
    success: string,
  ) {
    const provider = apiKeyManagerProvider.value;
    if (!provider) return;
    const providerId = provider.identity.id;
    const revision = ++requestRevision;
    apiKeyManagerOperation.value = operation;
    try {
      const updated = await action(providerId);
      if (!currentRequest(providerId, revision)) return;
      applyProvider(updated);
      Message.success(success);
    } catch (error) {
      if (currentRequest(providerId, revision)) Message.error(errorMessage(error));
      throw error;
    } finally {
      if (currentRequest(providerId, revision)) apiKeyManagerOperation.value = null;
    }
  }

  async function createManagedApiKey() {
    const provider = apiKeyManagerProvider.value;
    const name = apiKeyCreateName.value.trim();
    if (!provider) return;
    if (!name) return Message.warning("请填写 API 密钥名称");
    const providerId = provider.identity.id;
    const revision = ++requestRevision;
    apiKeyManagerOperation.value = "create";
    try {
      const keys = await options.createRemoteKey(providerId, name);
      if (!currentRequest(providerId, revision)) return;
      apiKeyManagerKeys.value = keys;
      applyProvider(currentProvider(providerId));
      apiKeyCreateVisible.value = false;
      apiKeyCreateName.value = "";
      Message.success("已创建站点 API Key");
    } catch (error) {
      if (currentRequest(providerId, revision)) Message.error(errorMessage(error));
    } finally {
      if (currentRequest(providerId, revision)) apiKeyManagerOperation.value = null;
    }
  }

  async function addLocalApiKey() {
    const key = apiKeyAddValue.value.trim();
    const remark = apiKeyAddRemark.value.trim();
    if (!key) return Message.warning("请填写 API Key");
    try {
      await runProviderMutation(
        "add",
        (providerId) => options.addLocalKey(providerId, key, remark),
        "API Key 已保存到当前卡片",
      );
      apiKeyAddVisible.value = false;
      apiKeyAddRemark.value = "";
      apiKeyAddValue.value = "";
    } catch {
      // Error is surfaced by runProviderMutation.
    }
  }

  async function saveManagedApiKeyRemark() {
    const target = apiKeyRemarkTarget.value;
    const remark = apiKeyRemarkValue.value.trim();
    if (!target) return;
    try {
      await runProviderMutation(
        "remark",
        (providerId) => options.setRemark(providerId, target.localId, remark),
        remark ? "已保存 API Key 本地备注" : "已清空 API Key 本地备注",
      );
      apiKeyRemarkVisible.value = false;
      apiKeyRemarkTarget.value = null;
      apiKeyRemarkValue.value = "";
    } catch {
      // Error is surfaced by runProviderMutation.
    }
  }

  async function setDefaultManagedApiKey(option: ProviderApiKeyOption) {
    const provider = apiKeyManagerProvider.value;
    return provider ? setDefaultKeyForProvider(provider, option) : false;
  }

  async function setDefaultKeyForProvider(provider: Provider, option: ProviderApiKeyOption) {
    if (!option.localId.trim() || !option.keyAvailable || !option.key.trim()) {
      Message.warning("该 API Key 未读取到完整值，无法设为当前调用 Key");
      return false;
    }
    const providerId = provider.identity.id;
    const revision = ++requestRevision;
    apiKeyManagerProvider.value = provider;
    apiKeyManagerOperation.value = "default";
    try {
      const updated = await options.setDefaultKey(providerId, option.localId);
      if (!currentRequest(providerId, revision)) return false;
      applyProvider(updated);
      Message.success(`本卡片将使用“${providerApiKeyDisplayName(option)}”发起默认请求`);
      return true;
    } catch (error) {
      if (currentRequest(providerId, revision)) Message.error(errorMessage(error));
      return false;
    } finally {
      if (currentRequest(providerId, revision)) apiKeyManagerOperation.value = null;
    }
  }

  async function copyManagedApiKey(option: ProviderApiKeyOption) {
    if (!option.keyAvailable || !option.key.trim()) {
      return Message.warning("该 API Key 未读取到完整值，无法复制");
    }
    try {
      await copyText(option.key);
      Message.success("已复制 API Key");
    } catch (error) {
      Message.error(errorMessage(error));
    }
  }

  function deleteManagedApiKey(option: ProviderApiKeyOption) {
    const provider = apiKeyManagerProvider.value;
    if (!provider) return;
    const remoteKey = Boolean(option.tokenId.trim());
    const remote = remoteKey && apiKeyRemoteManaged.value;
    const current = providerUsesApiKeyOption(provider, option);
    const boundAgents = options.getBoundAgentLabels?.(
      provider.identity.id,
      option.localId,
      current,
    ) ?? [];
    if (boundAgents.length > 0) {
      Modal.warning({
        title: "这把 API Key 正在使用",
        content: `${boundAgents.join("、")} 的默认配置仍绑定“${providerApiKeyDisplayName(option)}”。请先为这些 Agent 切换到其他 Key，再删除。`,
        okText: "知道了",
      });
      return;
    }
    const remainingUsable = apiKeyManagerKeys.value.filter((candidate) =>
      candidate.localId !== option.localId
      && candidate.keyAvailable
      && Boolean(candidate.key.trim()),
    );
    const impact = current
      ? remainingUsable.length > 0
        ? `删除后，当前调用 Key 会自动切换为“${providerApiKeyDisplayName(remainingUsable[0])}”。`
        : "删除后，本卡片将没有可用于模型请求或临时 CLI 的 API Key。"
      : "";
    const bindingImpact = current
      ? "如果其他外部工具仍在使用这把 Key，其配置不会被自动改写。"
      : "";
    Modal.confirm({
      title: remote ? "删除站点 API Key" : "移除本地 API Key",
      content: remote
        ? `确定从站点删除“${providerApiKeyDisplayName(option)}”吗？此操作会立即撤销站点令牌。${impact}${bindingImpact}`
        : remoteKey
          ? `确定仅从当前卡片移除“${providerApiKeyDisplayName(option)}”吗？站点上的令牌仍然有效；以后恢复账号凭据并重新同步时，它可能再次出现。${impact}${bindingImpact}`
          : `确定从本机移除“${providerApiKeyDisplayName(option)}”吗？不会删除站点上的令牌。${impact}${bindingImpact}`,
      okText: remote ? "删除" : "移除",
      cancelText: "取消",
      okButtonProps: { status: "danger" },
      onOk: async () => {
        if (remote) {
          const providerId = provider.identity.id;
          const revision = ++requestRevision;
          apiKeyManagerOperation.value = "delete";
          try {
            const keys = await options.deleteRemoteKey(providerId, option.tokenId);
            if (!currentRequest(providerId, revision)) return;
            apiKeyManagerKeys.value = keys;
            applyProvider(currentProvider(providerId));
            Message.success("已删除站点 API Key");
          } catch (error) {
            if (currentRequest(providerId, revision)) Message.error(errorMessage(error));
          } finally {
            if (currentRequest(providerId, revision)) apiKeyManagerOperation.value = null;
          }
          return;
        }
        try {
          await runProviderMutation(
            "delete",
            (providerId) => options.removeLocalKey(providerId, option.localId),
            "已移除本地 API Key",
          );
        } catch {
          // Error is surfaced by runProviderMutation.
        }
      },
    });
  }

  watch(
    () => {
      const providerId = apiKeyManagerProvider.value?.identity.id;
      return providerId
        ? options.providers.value.find((provider) => provider.identity.id === providerId)
        : undefined;
    },
    (provider) => {
      if (!provider || !apiKeyManagerProvider.value) return;
      apiKeyManagerProvider.value = provider;
      apiKeyManagerKeys.value = effectiveProviderApiKeyOptions(
        provider.auth.apiKey,
        provider.auth.apiKeyOptions || [],
      );
    },
    { flush: "sync" },
  );

  return {
    apiKeyManagerProvider,
    apiKeyManagerOperation,
    apiKeyManagerKeys,
    apiKeyRemoteManaged,
    apiKeyCreateVisible,
    apiKeyCreateName,
    apiKeyAddVisible,
    apiKeyAddRemark,
    apiKeyAddValue,
    apiKeyRemarkVisible,
    apiKeyRemarkValue,
    apiKeyRemarkTarget,
    bindApiKeyManager,
    closeApiKeyManager,
    openApiKeyCreatePanel,
    openApiKeyAddPanel,
    openApiKeyRemarkEditor,
    syncRemoteApiKeys,
    createManagedApiKey,
    addLocalApiKey,
    saveManagedApiKeyRemark,
    setDefaultManagedApiKey,
    setDefaultKeyForProvider,
    copyManagedApiKey,
    deleteManagedApiKey,
  };
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
