import { computed, onUnmounted, ref } from "vue";
import { agentEnvironmentErrorMessage, useAgentEnvironmentStore } from "../stores/agent-environment";
import { useAgentAssetConsole } from "./useAgentAssetConsole";
import type {
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

export function useAgentEnvironmentCenter(options: { settings?: AppSettings } = {}) {
  const store = useAgentEnvironmentStore();
  const cliRuntimeStore = useCliRuntimeStore();
  const selectedWorkspacePath = ref<string | undefined>(undefined);
  const deepScanState = ref<"idle" | "scanning" | "ready" | "error">("idle");
  const deepScanResult = ref<CliEnvironmentProbeResult | null>(null);
  const deepScanError = ref("");
  const deepScanSnapshot = ref<CliEnvironmentSettingsSnapshot | null>(null);
  let deepScanRequestId = 0;
  let activeStoreProbeRequestId: number | null = null;
  let disposed = false;

  const inventory = computed(() => store.inventory(selectedWorkspacePath.value));
  const assets = useAgentAssetConsole({ inventory, workspace: selectedWorkspacePath });
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
    deepScanState.value = "scanning";
    const pending = cliRuntimeStore.probeCliTools(true);
    const storeProbeRequestId = cliRuntimeStore.cliEnvironmentRequestId;
    activeStoreProbeRequestId = storeProbeRequestId;
    try {
      const result = await pending;
      if (disposed || requestId !== deepScanRequestId) return null;
      deepScanResult.value = result;
      deepScanState.value = "ready";
      return result;
    } catch (error) {
      if (disposed || requestId !== deepScanRequestId) return null;
      deepScanState.value = "error";
      deepScanError.value = agentEnvironmentErrorMessage(error);
      return null;
    } finally {
      if (requestId === deepScanRequestId && activeStoreProbeRequestId === storeProbeRequestId) {
        activeStoreProbeRequestId = null;
      }
      if (!disposed && requestId === deepScanRequestId && deepScanState.value === "scanning") {
        deepScanState.value = "idle";
      }
    }
  }

  function cancelDeepScan() {
    deepScanRequestId += 1;
    if (activeStoreProbeRequestId !== null) {
      cliRuntimeStore.cancelCliToolsProbe(activeStoreProbeRequestId);
    }
    activeStoreProbeRequestId = null;
    deepScanState.value = "idle";
    deepScanResult.value = null;
    deepScanError.value = "";
    deepScanSnapshot.value = null;
  }

  function canAdoptDeepScanCandidate(tool: CliToolProbeResult) {
    const settings = options.settings;
    const snapshot = deepScanSnapshot.value;
    return deepScanState.value === "ready"
      && Boolean(settings && snapshot && tool.available && tool.path.trim())
      && agentCliPathsMatchSnapshot(settings!, snapshot!);
  }

  function selectWorkspace(path?: string) {
    if (selectedWorkspacePath.value === path) return;
    selectedWorkspacePath.value = path;
  }

  onUnmounted(() => {
    disposed = true;
    cancelDeepScan();
  });

  return {
    inventory,
    assets,
    deepScanState,
    deepScanCandidates,
    deepScanError,
    deepScanDraftChanged,
    startDeepScan,
    cancelDeepScan,
    canAdoptDeepScanCandidate,
    selectWorkspace,
  };
}
