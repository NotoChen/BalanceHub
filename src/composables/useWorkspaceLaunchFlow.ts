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
import { withTimeout } from "../utils/promise-timeout.ts";
import { waitForTemporaryCliStart } from "../utils/temporary-cli-launch.ts";
import {
  isProviderApiKeyUsable,
  providerApiKeyOptionMatches,
} from "../utils/provider-api-key-options.ts";
import { providerDisplayLabel } from "../utils/provider-display.ts";

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
  apiKeyLocalId: Ref<string>;
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
  notify?: Partial<LaunchNotifications>;
}

interface LaunchNotifications {
  success: (message: string) => void;
  warning: (message: string) => void;
  error: (message: string) => void;
}

type WorkspaceLaunchState =
  | { phase: "idle" }
  | { phase: "previewing"; input: TemporaryCliLaunchInput }
  | { phase: "confirming"; input: TemporaryCliLaunchInput; preview: TemporaryCliLaunchPreview };

export interface TemporaryCliLaunchTask {
  id: string;
  title: string;
  detail: string;
  status: "running" | "success" | "failed";
  startedAt: number;
  finishedAt?: number;
  error?: string;
}

const PREVIEW_TIMEOUT_MS = 45_000;
const LAUNCH_DISPATCH_TIMEOUT_MS = 60_000;
const MAX_FINISHED_LAUNCH_TASKS = 12;

export function useWorkspaceLaunchFlow(options: UseWorkspaceLaunchFlowOptions) {
  const workspaceLaunchState = ref<WorkspaceLaunchState>({ phase: "idle" });
  const temporaryCliLaunchTasks = ref<TemporaryCliLaunchTask[]>([]);
  let previewRequestId = 0;
  let launchTaskCounter = 0;
  const notify: LaunchNotifications = {
    success: options.notify?.success ?? ((message) => Message.success(message)),
    warning: options.notify?.warning ?? ((message) => Message.warning(message)),
    error: options.notify?.error ?? ((message) => Message.error(message)),
  };
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
      || workspaceLaunchState.value.phase !== "idle"
    ) {
      return null;
    }
    if (
      !options.cliOptions.value.some((option) => option.value === options.cliKind.value)
      || !options.terminalOptions.value.some((option) => option.value === options.terminalKind.value)
    ) {
      return failLaunchInput("未检测到可用的 Agent 或终端");
    }

    const selectedKey = options.apiKeys.value.find((option) =>
      providerApiKeyOptionMatches(option, options.apiKeyLocalId.value),
    );
    if (selectedKey && !isProviderApiKeyUsable(selectedKey)) {
      return failLaunchInput("所选 API Key 未读取到完整值，请刷新密钥列表或改选其他 Key");
    }
    const apiKey = selectedKey?.key || provider.auth.apiKey.trim();
    const model = options.cliTool.value?.capabilities.modelSelection
      ? options.sessionMode.value === "new"
        ? (options.selectedModel.value.trim() || provider.cli.preferredModel.trim())
        : options.selectedModel.value.trim()
      : "";
    const sessionName = options.canNameSession.value ? options.sessionName.value.trim() : "";
    const cliPath = options.cliTool.value?.path.trim() || "";
    const resumeId = options.sessionMode.value === "history"
      ? options.selectedResumeId.value.trim()
      : "";
    if (!cliPath) {
      return failLaunchInput("所选 Agent CLI 缺少可用路径，请重新扫描后再试");
    }
    if (!apiKey || apiKey.includes("*")) {
      return failLaunchInput("请选择一个可用的 API Key");
    }
    if (options.sessionMode.value === "history" && !resumeId) {
      return failLaunchInput("请选择一个历史会话后再启动");
    }

    return {
      providerId: provider.identity.id,
      cliKind: options.cliKind.value,
      cliPath,
      workdir,
      apiKey,
      // A synthetic current-config option has no local identity. Its full key
      // remains in `apiKey`; never send that secret as a local-id selector.
      apiKeyLocalId: selectedKey?.localId || "",
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

    const requestId = ++previewRequestId;
    workspaceLaunchState.value = { phase: "previewing", input };
    options.error.value = "";
    try {
      const preview = await withTimeout(
        options.preview(input),
        PREVIEW_TIMEOUT_MS,
        "生成临时 CLI 启动预览超时，请检查 Agent CLI 与终端状态",
      );
      if (
        requestId !== previewRequestId
        || workspaceLaunchState.value.phase !== "previewing"
      ) {
        return;
      }
      workspaceLaunchState.value = {
        phase: "confirming",
        input: { ...input, cliPath: preview.cliPath },
        preview,
      };
    } catch (error) {
      if (
        requestId !== previewRequestId
        || workspaceLaunchState.value.phase !== "previewing"
      ) {
        return;
      }
      workspaceLaunchState.value = { phase: "idle" };
      reportPreviewFailure(errorMessage(error));
    }
  }

  function confirmWorkspaceLaunch() {
    if (workspaceLaunchState.value.phase !== "confirming") return;

    const input = workspaceLaunchState.value.input;
    const provider = options.provider.value;
    if (!provider) {
      workspaceLaunchState.value = { phase: "idle" };
      return;
    }
    const cliLabel = agentCliLabel(options.cliProbe.value, input.cliKind);
    options.error.value = "";
    workspaceLaunchState.value = { phase: "idle" };
    options.visible.value = false;
    const task = beginLaunchTask(cliLabel, providerDisplayLabel(provider));
    void launchInBackground(task.id, input, cliLabel);
  }

  async function launchInBackground(
    taskId: string,
    input: TemporaryCliLaunchInput,
    cliLabel: string,
  ) {
    try {
      const result = await withTimeout(
        options.launch(input),
        LAUNCH_DISPATCH_TIMEOUT_MS,
        `派发 ${cliLabel} 启动命令超时，请检查终端自动化权限`,
      );
      updateLaunchTask(taskId, {
        detail: `${result.instance.terminalName} 已打开，正在确认 ${cliLabel} 进程`,
      });
      await waitForTemporaryCliStart(result.instance.id, options.getInstance);
      if (result.workspaceError) {
        finishLaunchTask(
          taskId,
          "success",
          `${cliLabel} 已启动，但工作空间记录失败：${result.workspaceError}`,
        );
        notify.warning(`${cliLabel} 已启动，但工作空间记录失败：${result.workspaceError}`);
      } else {
        finishLaunchTask(taskId, "success", `${cliLabel} 已启动`);
        notify.success(`已在所选工作空间启动 ${cliLabel}`);
      }
    } catch (error) {
      const message = errorMessage(error);
      finishLaunchTask(taskId, "failed", `${cliLabel} 启动失败`, message);
      notify.error(`${cliLabel} 启动失败：${message}`);
    }
  }

  function resetWorkspaceLaunch() {
    previewRequestId += 1;
    workspaceLaunchState.value = { phase: "idle" };
  }

  function failLaunchInput(message: string) {
    options.error.value = message;
    notify.warning(message);
    return null;
  }

  function reportPreviewFailure(message: string) {
    options.error.value = message;
    notify.error(message);
  }

  function beginLaunchTask(cliLabel: string, providerName: string) {
    launchTaskCounter += 1;
    const task: TemporaryCliLaunchTask = {
      id: `temporary-cli-launch-${Date.now().toString(36)}-${launchTaskCounter.toString(36)}`,
      title: `启动 ${cliLabel}`,
      detail: `正在为“${providerName}”准备终端环境`,
      status: "running",
      startedAt: Date.now(),
    };
    temporaryCliLaunchTasks.value = [task, ...temporaryCliLaunchTasks.value];
    trimLaunchTasks();
    return task;
  }

  function updateLaunchTask(id: string, patch: Partial<TemporaryCliLaunchTask>) {
    temporaryCliLaunchTasks.value = temporaryCliLaunchTasks.value.map((task) =>
      task.id === id ? { ...task, ...patch } : task,
    );
  }

  function finishLaunchTask(
    id: string,
    status: "success" | "failed",
    detail: string,
    error?: string,
  ) {
    updateLaunchTask(id, {
      status,
      detail,
      error,
      finishedAt: Date.now(),
    });
    trimLaunchTasks();
  }

  function trimLaunchTasks() {
    const running = temporaryCliLaunchTasks.value.filter((task) => task.status === "running");
    const finished = temporaryCliLaunchTasks.value
      .filter((task) => task.status !== "running")
      .sort((left, right) => (right.finishedAt ?? 0) - (left.finishedAt ?? 0))
      .slice(0, MAX_FINISHED_LAUNCH_TASKS);
    temporaryCliLaunchTasks.value = [...running, ...finished];
  }

  return {
    workspaceLaunchPreviewVisible,
    workspaceLaunchPreviewLoading,
    workspaceLaunchPreview,
    temporaryCliLaunchTasks,
    launchWorkspace,
    confirmWorkspaceLaunch,
    resetWorkspaceLaunch,
  };
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
