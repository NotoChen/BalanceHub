import { computed, onUnmounted, ref } from "vue";
import { Message } from "@arco-design/web-vue";
import { useClipboard } from "./useClipboard";
import { agentEnvironmentKey, useAgentEnvironmentStore } from "../stores/agent-environment";
import type {
  AgentAssetCategory,
  AgentAssetReadResult,
  AgentAssetRecord,
  AgentInstallation,
  AgentCliKind,
  AppSettings,
  CliEnvironmentProbeResult,
  CliToolProbeResult,
} from "../stores/provider-types";
import { useCliRuntimeStore } from "../stores/cli-runtime";
import {
  agentCliPathsMatchSnapshot,
  captureCliEnvironmentSettings,
  type CliEnvironmentSettingsSnapshot,
} from "../utils/cli-environment";

export type AgentEnvironmentDetailTab = "overview" | "assets" | "configuration";

export const agentAssetCategoryLabels: Record<AgentAssetCategory, string> = {
  config: "配置",
  skill: "Skills",
  plugin: "插件",
  extension: "扩展",
  mcp: "MCP",
  hook: "Hook",
  statusUi: "Status UI",
};

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

export function useAgentEnvironmentCenter(options: { settings?: AppSettings } = {}) {
  const store = useAgentEnvironmentStore();
  const cliRuntimeStore = useCliRuntimeStore();
  const clipboard = useClipboard();
  const selectedInstallationId = ref<string | null>(null);
  const selectedWorkspacePath = ref<string | undefined>(undefined);
  const activeDetailTab = ref<AgentEnvironmentDetailTab>("overview");
  const assetCategory = ref<AgentAssetCategory | "all">("all");
  const assetQuery = ref("");
  const selectedConfigFileId = ref<string | null>(null);
  const configPreview = ref<AgentAssetReadResult | null>(null);
  const configPreviewState = ref<"idle" | "loading" | "refreshing" | "ready" | "error">("idle");
  const configPreviewError = ref("");
  const copiedPathId = ref<string | null>(null);
  const deepScanState = ref<"idle" | "scanning" | "ready" | "error">("idle");
  const deepScanResult = ref<CliEnvironmentProbeResult | null>(null);
  const deepScanError = ref("");
  const deepScanSnapshot = ref<CliEnvironmentSettingsSnapshot | null>(null);
  const adoptedDeepScanKinds = ref<AgentCliKind[]>([]);
  let copiedPathTimer: ReturnType<typeof window.setTimeout> | null = null;
  let inventoryRequestId = 0;
  let versionRequestId = 0;
  let previewRequestId = 0;
  let deepScanRequestId = 0;
  let activeStoreProbeRequestId: number | null = null;
  let disposed = false;

  const inventory = computed(() => store.inventory(selectedWorkspacePath.value));
  const workspaceKey = computed(() => agentEnvironmentKey(selectedWorkspacePath.value));
  const inventoryState = computed(() => store.inventoryState[workspaceKey.value] ?? "idle");
  const inventoryError = computed(() => store.inventoryErrors[workspaceKey.value] ?? null);
  const versionState = computed(() => store.versionState[workspaceKey.value] ?? "idle");
  const versionError = computed(() => store.versionErrors[workspaceKey.value] ?? null);
  const selectedInstallation = computed<AgentInstallation | null>(() =>
    inventory.value?.installations.find((item) => item.id === selectedInstallationId.value) ?? null,
  );
  const installationAssets = computed(() => {
    const kind = selectedInstallation.value?.agentKind;
    return (inventory.value?.assets ?? []).filter((asset) => !kind || asset.agentKind === kind);
  });
  const selectedAssets = computed(() => {
    const query = assetQuery.value.trim().toLocaleLowerCase();
    return installationAssets.value.filter((asset) => {
      if (assetCategory.value !== "all" && asset.category !== assetCategory.value) return false;
      return matchesAssetQuery(asset, query);
    });
  });
  const configurationAssets = computed(() => {
    return installationAssets.value.filter((asset) => asset.category === "config");
  });
  const selectedCapabilities = computed(() =>
    inventory.value?.capabilities.find(
      (capability) => capability.agentKind === selectedInstallation.value?.agentKind,
    )?.assets ?? [],
  );
  const selectedCategorySupported = computed(() =>
    assetCategory.value === "all"
      || selectedCapabilities.value.some((capability) => capability.category === assetCategory.value),
  );
  const configurationSupported = computed(() =>
    selectedCapabilities.value.some((capability) => capability.category === "config"),
  );
  const deepScanCandidates = computed(() =>
    (deepScanResult.value?.tools ?? []).filter(
      (tool) => tool.available && Boolean(tool.path.trim()),
    ),
  );
  const deepScanDraftChanged = computed(() => {
    const snapshot = deepScanSnapshot.value;
    return Boolean(options.settings && snapshot)
      && !agentCliPathsMatchSnapshot(options.settings!, snapshot!);
  });

  async function loadInventory(forceRefresh = false) {
    const requestId = ++inventoryRequestId;
    try {
      const result = await store.loadInventory(selectedWorkspacePath.value, forceRefresh);
      if (disposed || requestId !== inventoryRequestId) return null;
      if (
        selectedInstallationId.value
        && !result.installations.some((item) => item.id === selectedInstallationId.value)
      ) {
        selectedInstallationId.value = null;
        activeDetailTab.value = "overview";
      }
      if (
        selectedConfigFileId.value
        && !result.assets.some((asset) => asset.stableId === selectedConfigFileId.value)
      ) {
        resetConfigPreview();
      }
      return result;
    } catch (error) {
      if (!disposed && requestId === inventoryRequestId) {
        Message.error(`Agent 环境盘点失败：${errorMessage(error)}`);
      }
      return null;
    }
  }

  async function refreshVersions() {
    const requestId = ++versionRequestId;
    try {
      const result = await store.refreshLatestVersions(selectedWorkspacePath.value);
      return disposed || requestId !== versionRequestId ? null : result;
    } catch (error) {
      if (!disposed && requestId === versionRequestId) {
        Message.error(`版本检查失败：${errorMessage(error)}`);
      }
      return null;
    }
  }

  async function startDeepScan() {
    if (deepScanState.value === "scanning") return null;
    const settings = options.settings;
    if (!settings) {
      deepScanState.value = "error";
      deepScanError.value = "当前设置草稿不可用，无法采用扫描结果";
      return null;
    }

    const requestId = ++deepScanRequestId;
    deepScanSnapshot.value = captureCliEnvironmentSettings(settings);
    deepScanResult.value = null;
    deepScanError.value = "";
    adoptedDeepScanKinds.value = [];
    deepScanState.value = "scanning";
    const pending = cliRuntimeStore.probeCliTools(true);
    activeStoreProbeRequestId = cliRuntimeStore.cliEnvironmentRequestId;
    try {
      const result = await pending;
      if (disposed || requestId !== deepScanRequestId) return null;
      deepScanResult.value = result;
      deepScanState.value = "ready";
      return result;
    } catch (error) {
      if (disposed || requestId !== deepScanRequestId) return null;
      deepScanState.value = "error";
      deepScanError.value = errorMessage(error);
      return null;
    } finally {
      if (activeStoreProbeRequestId === cliRuntimeStore.cliEnvironmentRequestId) {
        activeStoreProbeRequestId = null;
      }
      if (!disposed && requestId === deepScanRequestId && deepScanState.value === "scanning") {
        deepScanState.value = "idle";
      }
    }
  }

  function cancelDeepScan() {
    if (deepScanState.value !== "scanning") return;
    deepScanRequestId += 1;
    if (activeStoreProbeRequestId !== null) {
      cliRuntimeStore.cancelCliToolsProbe(activeStoreProbeRequestId);
    }
    activeStoreProbeRequestId = null;
    deepScanState.value = "idle";
    deepScanResult.value = null;
    deepScanError.value = "";
    deepScanSnapshot.value = null;
    adoptedDeepScanKinds.value = [];
  }

  function canAdoptDeepScanCandidate(tool: CliToolProbeResult) {
    const settings = options.settings;
    const snapshot = deepScanSnapshot.value;
    return deepScanState.value === "ready"
      && Boolean(settings && snapshot && tool.available && tool.path.trim())
      && !adoptedDeepScanKinds.value.includes(tool.kind)
      && agentCliPathsMatchSnapshot(settings!, snapshot!);
  }

  function isDeepScanCandidateAdopted(tool: CliToolProbeResult) {
    return adoptedDeepScanKinds.value.includes(tool.kind)
      && options.settings?.agentCliPaths[tool.kind] === tool.path;
  }

  function adoptDeepScanCandidate(tool: CliToolProbeResult) {
    const settings = options.settings;
    const snapshot = deepScanSnapshot.value;
    if (!settings || !snapshot || !tool.available || !tool.path.trim()) return false;
    if (!agentCliPathsMatchSnapshot(settings, snapshot)) {
      return false;
    }
    settings.agentCliPaths[tool.kind] = tool.path;
    if (!adoptedDeepScanKinds.value.includes(tool.kind)) {
      adoptedDeepScanKinds.value = [...adoptedDeepScanKinds.value, tool.kind];
    }
    return true;
  }

  function openInstallation(installationId: string) {
    cancelSelectedPreview();
    selectedInstallationId.value = installationId;
    activeDetailTab.value = "overview";
    assetCategory.value = "all";
    assetQuery.value = "";
    resetConfigPreview();
  }

  function closeInstallation() {
    cancelSelectedPreview();
    selectedInstallationId.value = null;
    resetConfigPreview();
  }

  async function selectConfig(asset: AgentAssetRecord) {
    if (asset.category !== "config") return;
    const sameAsset = selectedConfigFileId.value === asset.stableId;
    selectedConfigFileId.value = asset.stableId;
    if (!sameAsset) configPreview.value = null;
    configPreviewError.value = "";
    configPreviewState.value = configPreview.value ? "refreshing" : "loading";
    const requestId = ++previewRequestId;
    try {
      const result = await store.readConfigPreview(asset.stableId, selectedWorkspacePath.value);
      if (disposed || requestId !== previewRequestId || selectedConfigFileId.value !== asset.stableId) return;
      configPreview.value = result;
      configPreviewState.value = "ready";
    } catch (error) {
      if (disposed || requestId !== previewRequestId || selectedConfigFileId.value !== asset.stableId) return;
      configPreviewState.value = "error";
      configPreviewError.value = errorMessage(error);
    }
  }

  async function openAsset(asset: AgentAssetRecord) {
    try {
      await store.openAsset(
        asset.stableId,
        selectedWorkspacePath.value,
        "asset",
      );
    } catch (error) {
      Message.error(`无法打开 Agent 资产：${errorMessage(error)}`);
    }
  }

  async function copyPath(asset: AgentAssetRecord) {
    if (!asset.path) return;
    try {
      await clipboard.copyText(asset.path);
      copiedPathId.value = asset.stableId;
      if (copiedPathTimer !== null) window.clearTimeout(copiedPathTimer);
      copiedPathTimer = window.setTimeout(() => {
        if (copiedPathId.value === asset.stableId) copiedPathId.value = null;
        copiedPathTimer = null;
      }, 1500);
    } catch (error) {
      Message.error(`复制路径失败：${errorMessage(error)}`);
    }
  }

  function selectWorkspace(path?: string) {
    if (selectedWorkspacePath.value === path) return;
    cancelSelectedPreview();
    selectedWorkspacePath.value = path;
    selectedInstallationId.value = null;
    resetConfigPreview();
    inventoryRequestId += 1;
    versionRequestId += 1;
  }

  function setDetailTab(tab: AgentEnvironmentDetailTab) {
    activeDetailTab.value = tab;
  }

  onUnmounted(() => {
    disposed = true;
    if (deepScanState.value === "scanning" && activeStoreProbeRequestId !== null) {
      cliRuntimeStore.cancelCliToolsProbe(activeStoreProbeRequestId);
    }
    deepScanRequestId += 1;
    activeStoreProbeRequestId = null;
    deepScanState.value = "idle";
    deepScanResult.value = null;
    deepScanError.value = "";
    deepScanSnapshot.value = null;
    adoptedDeepScanKinds.value = [];
    cancelSelectedPreview();
    if (copiedPathTimer !== null) window.clearTimeout(copiedPathTimer);
    inventoryRequestId += 1;
    versionRequestId += 1;
    previewRequestId += 1;
  });

  return {
    inventory,
    inventoryState,
    inventoryError,
    versionState,
    versionError,
    selectedInstallation,
    selectedInstallationId,
    selectedWorkspacePath,
    activeDetailTab,
    assetCategory,
    assetQuery,
    selectedAssets,
    installationAssetCount: computed(() => installationAssets.value.length),
    configurationAssets,
    selectedCategorySupported,
    configurationSupported,
    selectedConfigFileId,
    configPreview,
    configPreviewState,
    configPreviewError,
    copiedPathId,
    deepScanState,
    deepScanResult,
    deepScanCandidates,
    deepScanError,
    deepScanDraftChanged,
    startDeepScan,
    cancelDeepScan,
    canAdoptDeepScanCandidate,
    isDeepScanCandidateAdopted,
    adoptDeepScanCandidate,
    loadInventory,
    refreshVersions,
    openInstallation,
    closeInstallation,
    selectWorkspace,
    setDetailTab,
    selectConfig,
    openAsset,
    copyPath,
  };

  function cancelSelectedPreview() {
    previewRequestId += 1;
  }

  function resetConfigPreview() {
    selectedConfigFileId.value = null;
    configPreview.value = null;
    configPreviewState.value = "idle";
    configPreviewError.value = "";
    previewRequestId += 1;
  }
}

function matchesAssetQuery(asset: AgentAssetRecord, query: string) {
  if (!query) return true;
  return [asset.label, asset.nativeId, asset.path ?? "", asset.sourceId, asset.diagnostics.join(" ")]
    .join(" ")
    .toLocaleLowerCase()
    .includes(query);
}
