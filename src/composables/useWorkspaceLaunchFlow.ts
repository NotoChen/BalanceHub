import { computed, ref, type Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import type { SelectOption } from "../utils/liveness-options.ts";
import type {
  AgentCliKind,
  CliEnvironmentProbeResult,
  CliToolProbeResult,
  Provider,
  ProviderApiKeyOption,
  TemporaryCliInstance,
  TemporaryCliLaunchInput,
  TemporaryCliLaunchPreview,
  TemporaryCliLaunchResult,
  TemporaryCliSessionMode,
  TemporaryCliTerminalKind,
  WorkspaceDirectoryListing,
} from "../stores/providers";
import { agentCliLabel } from "../utils/cli-environment.ts";
import { waitForTemporaryCliStart } from "../utils/temporary-cli-launch.ts";

interface UseWorkspaceLaunchFlowOptions {
  visible: Ref<boolean>;
  provider: Ref<Provider | null>;
  cliKind: Ref<AgentCliKind>;
  cliOptions: Ref<SelectOption<AgentCliKind>[]>;
  cliTool: Ref<CliToolProbeResult | null>;
  cliProbe: Ref<CliEnvironmentProbeResult | null>;
  terminalKind: Ref<TemporaryCliTerminalKind>;
  terminalOptions: Ref<SelectOption<TemporaryCliTerminalKind>[]>;
  directory: Ref<WorkspaceDirectoryListing | null>;
  apiKeys: Ref<ProviderApiKeyOption[]>;
  apiKeyTokenId: Ref<string>;
  selectedModel: Ref<string>;
  sessionMode: Ref<TemporaryCliSessionMode>;
  sessionName: Ref<string>;
  canNameSession: Ref<boolean>;
  selectedResumeId: Ref<string>;
  selectedSessionTitle: Ref<string>;
  error: Ref<string>;
  launch: (input: TemporaryCliLaunchInput) => Promise<TemporaryCliLaunchResult>;
  preview: (input: TemporaryCliLaunchInput) => Promise<TemporaryCliLaunchPreview>;
  getInstance: (instanceId: string) => Promise<TemporaryCliInstance | null>;
}

type WorkspaceLaunchState =
  | { phase: "idle" }
  | { phase: "previewing"; input: TemporaryCliLaunchInput }
  | { phase: "confirming"; input: TemporaryCliLaunchInput; preview: TemporaryCliLaunchPreview }
  | { phase: "launching"; input: TemporaryCliLaunchInput }
  | { phase: "failed" };

export function useWorkspaceLaunchFlow(options: UseWorkspaceLaunchFlowOptions) {
  const workspaceLaunchState = ref<WorkspaceLaunchState>({ phase: "idle" });
  const workspaceLaunchingPath = computed(() =>
    workspaceLaunchState.value.phase === "launching"
      ? workspaceLaunchState.value.input.workdir
      : null,
  );
  const workspaceLaunchPreviewVisible = computed({
    get: () => workspaceLaunchState.value.phase === "confirming",
    set: (visible: boolean) => {
      if (!visible && workspaceLaunchState.value.phase === "confirming") {
        workspaceLaunchState.value = { phase: "idle" };
      }
    },
  });
  const workspaceLaunchPreviewLoading = computed(
    () => workspaceLaunchState.value.phase === "previewing",
  );
  const workspaceLaunchPreview = computed(() =>
    workspaceLaunchState.value.phase === "confirming"
      ? workspaceLaunchState.value.preview
      : null,
  );

  function buildLaunchInput(path?: string): TemporaryCliLaunchInput | null {
    const provider = options.provider.value;
    const workdir = (path || options.directory.value?.currentPath || "").trim();
    if (
      !provider
      || !workdir
      || ["previewing", "confirming", "launching"].includes(workspaceLaunchState.value.phase)
    ) {
      return null;
    }
    if (
      !options.cliOptions.value.some((option) => option.value === options.cliKind.value)
      || !options.terminalOptions.value.some((option) => option.value === options.terminalKind.value)
    ) {
      return failLaunchInput("未检测到可用的 Agent 或终端");
    }

    const selectedKey = options.apiKeys.value.find(
      (option) => option.tokenId === options.apiKeyTokenId.value,
    );
    const apiKey = selectedKey?.key || provider.auth.apiKey.trim();
    const model = options.cliTool.value?.capabilities.modelSelection
      ? options.sessionMode.value === "new"
        ? (options.selectedModel.value.trim() || provider.cli.preferredModel.trim())
        : options.selectedModel.value.trim()
      : "";
    const sessionName = options.canNameSession.value ? options.sessionName.value.trim() : "";
    const resumeId = options.sessionMode.value === "history"
      ? options.selectedResumeId.value.trim()
      : "";
    if (!apiKey) {
      return failLaunchInput("请选择一个可用的 API Key");
    }
    if (options.sessionMode.value === "history" && !resumeId) {
      return failLaunchInput("请选择一个历史会话后再启动");
    }

    return {
      providerId: provider.identity.id,
      cliKind: options.cliKind.value,
      workdir,
      apiKey,
      apiKeyTokenId: options.apiKeyTokenId.value,
      model,
      sessionMode: options.sessionMode.value,
      sessionName,
      resumeId,
      sessionTitle: options.sessionMode.value === "history"
        ? options.selectedSessionTitle.value.trim()
        : sessionName,
      terminalKind: options.terminalKind.value,
    };
  }

  async function launchWorkspace(path?: string) {
    const input = buildLaunchInput(path);
    if (!input) return;

    workspaceLaunchState.value = { phase: "previewing", input };
    options.error.value = "";
    try {
      const preview = await options.preview(input);
      if (
        workspaceLaunchState.value.phase !== "previewing"
        || workspaceLaunchState.value.input !== input
      ) {
        return;
      }
      workspaceLaunchState.value = { phase: "confirming", input, preview };
    } catch (error) {
      if (
        workspaceLaunchState.value.phase !== "previewing"
        || workspaceLaunchState.value.input !== input
      ) {
        return;
      }
      failLaunch(errorMessage(error));
    }
  }

  async function confirmWorkspaceLaunch() {
    if (workspaceLaunchState.value.phase !== "confirming") return;

    const input = workspaceLaunchState.value.input;
    if (!options.provider.value) {
      workspaceLaunchState.value = { phase: "idle" };
      return;
    }
    const cliLabel = agentCliLabel(options.cliProbe.value, input.cliKind);
    options.error.value = "";
    workspaceLaunchState.value = { phase: "launching", input };
    try {
      const result = await options.launch(input);
      await waitForTemporaryCliStart(result.instance.id, options.getInstance);
      if (result.workspaceError) {
        Message.warning(`${cliLabel} 已启动，但工作空间记录失败：${result.workspaceError}`);
      } else {
        Message.success(`已在所选工作空间启动 ${cliLabel}`);
      }
      options.visible.value = false;
      workspaceLaunchState.value = { phase: "idle" };
    } catch (error) {
      failLaunch(errorMessage(error));
    }
  }

  function resetWorkspaceLaunch() {
    workspaceLaunchState.value = { phase: "idle" };
  }

  function failLaunchInput(message: string) {
    options.error.value = message;
    Message.warning(message);
    return null;
  }

  function failLaunch(message: string) {
    options.error.value = message;
    workspaceLaunchState.value = { phase: "failed" };
    Message.error(message);
  }

  return {
    workspaceLaunchingPath,
    workspaceLaunchPreviewVisible,
    workspaceLaunchPreviewLoading,
    workspaceLaunchPreview,
    launchWorkspace,
    confirmWorkspaceLaunch,
    resetWorkspaceLaunch,
  };
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
