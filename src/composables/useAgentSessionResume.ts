import { computed, onScopeDispose, ref, watch, type Ref } from "vue";
import { useAgentSessionResumeStore } from "../stores/agent-session-resume";
import { useCliRuntimeStore } from "../stores/cli-runtime";
import { useSettingsStore } from "../stores/settings";
import { useWorkspaceStore } from "../stores/workspaces";
import type { AgentSessionResumeIntent, AgentSessionRow } from "../stores/agent-session-types";
import type { AgentInstallation, Provider, TemporaryCliTerminalKind } from "../stores/provider-types";
import { availableTerminalOptions } from "../utils/cli-environment";
import { providerDefaultApiKeyOption } from "../utils/provider-display";
import { withTimeout } from "../utils/promise-timeout";

interface AgentSessionResumeOptions {
  visible: Ref<boolean>;
  row: Ref<AgentSessionRow | null>;
  scopeRevision: Ref<string>;
  providers: Ref<readonly Provider[]>;
  installations: Ref<readonly AgentInstallation[]>;
  close: () => void;
}

/** Modal selection only. The persistent store owns submission and background work. */
export function useAgentSessionResume(options: AgentSessionResumeOptions) {
  const cli = useCliRuntimeStore();
  const settings = useSettingsStore();
  const workspaces = useWorkspaceStore();
  const tasks = useAgentSessionResumeStore();
  const intentKind = ref<AgentSessionResumeIntent["kind"]>("native");
  const providerId = ref("");
  const apiKeyLocalId = ref("");
  const cliPath = ref("");
  const terminalKind = ref<TemporaryCliTerminalKind>(settings.settings.temporaryCliTerminalKind);
  const probing = ref(false);
  const probeError = ref("");
  const confirming = ref(false);
  const selectedProvider = computed(() => options.providers.value.find((provider) => provider.identity.id === providerId.value) ?? null);
  const apiKeys = computed(() => selectedProvider.value?.auth.apiKeyOptions.filter((key) => key.localId.trim()) ?? []);
  const terminalOptions = computed(() => availableTerminalOptions(cli.terminalEnvironmentProbe));
  const cliOptions = computed(() => {
    const kind = options.row.value?.session.cliKind;
    if (!kind) return [];
    const choices = new Map<string, { path: string; label: string; version: string | null; preferred: boolean }>();
    const tool = cli.cliEnvironmentProbe?.tools.find((candidate) => candidate.kind === kind && candidate.available);
    const preferred = tool?.path || settings.settings.agentCliPaths[kind] || "";
    if (tool?.path) choices.set(tool.path, { path: tool.path, label: tool.label, version: tool.version, preferred: true });
    for (const installation of options.installations.value) {
      if (installation.agentKind !== kind || installation.availability !== "available" || !installation.executablePath) continue;
      if (!choices.has(installation.executablePath)) choices.set(installation.executablePath, {
        path: installation.executablePath, label: installation.label, version: installation.installedVersion,
        preferred: installation.executablePath === preferred,
      });
    }
    return [...choices.values()];
  });
  const unavailableReason = computed(() => {
    const row = options.row.value;
    if (!row) return "";
    if (!row.session.canResume) return row.resumeReason || "该原生会话不支持继续";
    if (tasks.isReserved(row.sessionRef)) return "该会话已有待确认的继续任务，请在后台任务中查看状态";
    if (!cliOptions.value.some((choice) => choice.path === cliPath.value)) return probing.value ? "正在检测可用 CLI" : "没有可用的 Agent CLI；历史仍可查看";
    if (!terminalOptions.value.some((choice) => choice.value === terminalKind.value)) return probing.value ? "正在检测终端" : "没有可用终端，请刷新检测";
    if (intentKind.value === "provider" && !selectedProvider.value) return "请选择要使用的中转站";
    return "";
  });
  const canConfirm = computed(() => options.visible.value && Boolean(options.row.value && options.scopeRevision.value)
    && !confirming.value && !unavailableReason.value);
  let revision = 0;
  let disposed = false;
  let ownedCliProbe: number | null = null;
  let ownedTerminalProbe: number | null = null;

  function invalidateProbe() {
    revision += 1;
    if (ownedCliProbe !== null) cli.cancelCliToolsProbe(ownedCliProbe);
    if (ownedTerminalProbe !== null) cli.cancelTerminalsProbe(ownedTerminalProbe);
    ownedCliProbe = null;
    ownedTerminalProbe = null;
    probing.value = false;
  }

  function selectDefaults() {
    if (!cliOptions.value.some((choice) => choice.path === cliPath.value)) {
      cliPath.value = cliOptions.value.find((choice) => choice.preferred)?.path ?? cliOptions.value[0]?.path ?? "";
    }
    if (!terminalOptions.value.some((choice) => choice.value === terminalKind.value)) {
      terminalKind.value = terminalOptions.value.find((choice) => choice.value === settings.settings.temporaryCliTerminalKind)?.value
        ?? terminalOptions.value[0]?.value ?? settings.settings.temporaryCliTerminalKind;
    }
  }

  async function refreshEnvironment() {
    if (!options.visible.value || disposed) return;
    invalidateProbe();
    const request = revision;
    probing.value = true;
    probeError.value = "";
    const cliRead = cli.probeCliTools(true);
    ownedCliProbe = cli.cliEnvironmentRequestId;
    const terminalRead = cli.probeTerminals();
    ownedTerminalProbe = cli.terminalEnvironmentRequestId;
    try {
      const results = await Promise.allSettled([
        withTimeout(cliRead, 45_000, "检测 Agent CLI 超时"),
        withTimeout(terminalRead, 20_000, "检测终端超时"),
      ]);
      if (disposed || request !== revision || !options.visible.value) return;
      const messages: string[] = [];
      for (const result of results) if (result.status === "rejected") messages.push(result.reason instanceof Error ? result.reason.message : String(result.reason));
      probeError.value = messages.join("；");
      selectDefaults();
    } finally {
      if (request === revision) {
        if (ownedCliProbe !== null) cli.cancelCliToolsProbe(ownedCliProbe);
        if (ownedTerminalProbe !== null) cli.cancelTerminalsProbe(ownedTerminalProbe);
        ownedCliProbe = null;
        ownedTerminalProbe = null;
        probing.value = false;
      }
    }
  }

  function selectProvider(id: string) {
    providerId.value = id;
    const provider = selectedProvider.value;
    const preference = workspaces.temporaryCliPreferences.find((item) => item.providerId === id && item.cliKind === options.row.value?.session.cliKind);
    apiKeyLocalId.value = preference && apiKeys.value.some((key) => key.localId === preference.apiKeyLocalId)
      ? preference.apiKeyLocalId : provider ? providerDefaultApiKeyOption(provider)?.localId || "" : "";
  }

  function confirm() {
    const row = options.row.value;
    if (!row || !canConfirm.value) return;
    const intent: AgentSessionResumeIntent = intentKind.value === "native"
      ? { kind: "native" }
      : { kind: "provider", providerId: providerId.value, apiKeyLocalId: apiKeyLocalId.value || null };
    const input = { sessionRef: row.sessionRef, scopeRevision: options.scopeRevision.value, cliPath: cliPath.value, terminalKind: terminalKind.value, intent };
    confirming.value = true;
    invalidateProbe();
    options.close();
    void tasks.submit(input);
  }

  watch(() => [options.visible.value, options.row.value?.sessionRef, options.scopeRevision.value] as const, () => {
    invalidateProbe();
    confirming.value = false;
    if (!options.visible.value || !options.row.value) return;
    intentKind.value = "native";
    providerId.value = "";
    apiKeyLocalId.value = "";
    cliPath.value = "";
    terminalKind.value = settings.settings.temporaryCliTerminalKind;
    probeError.value = "";
    selectDefaults();
    void refreshEnvironment();
  }, { immediate: true, flush: "sync" });
  watch(apiKeys, (keys) => { if (apiKeyLocalId.value && !keys.some((key) => key.localId === apiKeyLocalId.value)) apiKeyLocalId.value = ""; });
  onScopeDispose(() => { disposed = true; invalidateProbe(); });

  return { tasks, intentKind, providerId, apiKeyLocalId, selectedProvider, apiKeys, cliPath, cliOptions,
    terminalKind, terminalOptions, probing, probeError, unavailableReason, canConfirm, refreshEnvironment, selectProvider, confirm };
}
