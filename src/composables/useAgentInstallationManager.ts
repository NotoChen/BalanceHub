import { computed, onScopeDispose, reactive, ref, watch, type Ref, type UnwrapNestedRefs } from "vue";
import { Message } from "@arco-design/web-vue";
import { useAgentLifecycleConsole } from "./useAgentLifecycleConsole";
import type { useAgentEnvironmentCenter } from "./useAgentEnvironmentCenter";
import { useAgentLifecycleStore } from "../stores/agent-lifecycle";
import { useAgentOverviewStore } from "../stores/agent-overview";
import { useCliRuntimeStore } from "../stores/cli-runtime";
import { useSettingsStore } from "../stores/settings";
import { agentEnvironmentErrorMessage } from "../stores/agent-environment";
import type { AgentCliDescriptor, AgentCliKind, CliToolProbeResult } from "../stores/provider-types";
import type { AgentLifecycleTarget, AgentLifecycleVersionRefresh } from "../stores/agent-lifecycle-types";
import { withTimeout } from "../utils/promise-timeout";

export function useAgentInstallationManager(options: {
  agents: Ref<AgentCliDescriptor[]>;
  workspace: Ref<string | undefined>;
  navigationRevision: Ref<number>;
  environment: UnwrapNestedRefs<ReturnType<typeof useAgentEnvironmentCenter>>;
  inspectHooks: (kind: AgentCliKind) => Promise<unknown>;
}) {
  const store = useAgentLifecycleStore();
  const settings = useSettingsStore();
  const cli = useCliRuntimeStore();
  const overview = useAgentOverviewStore();
  const lifecycle = reactive(useAgentLifecycleConsole(options.navigationRevision));
  const label = computed(() => options.agents.value.find((agent) => agent.kind === lifecycle.agentKind)?.label ?? "Agent");
  const pathDraft = ref("");
  const savingPath = ref<AgentCliKind | null>(null);
  const pathErrors = ref<Partial<Record<AgentCliKind, string>>>({});
  const pathError = computed(() => lifecycle.agentKind ? pathErrors.value[lifecycle.agentKind] ?? "" : "");
  const diagnosticInstallationId = ref<string | null>(null);
  const selectedPaths = computed(() => Object.fromEntries((cli.cliEnvironmentProbe?.tools ?? []).map((tool) => [tool.kind, tool.path])));
  const launchPath = computed(() => {
    const kind = lifecycle.agentKind;
    if (!kind) return null;
    const current = store.catalog?.targets.find((target) => target.agentKind === kind && target.isCurrent);
    if (current) return current.installation.executablePath;
    const probe = overview.get(kind, options.workspace.value)?.probe ?? cli.cliEnvironmentProbe?.tools.find((tool) => tool.kind === kind);
    return probe?.available ? probe.path : null;
  });
  let viewRevision = 0;
  let pathRequest = 0;
  let activeProbeRequest: number | null = null;
  let disposed = false;

  function canAdopt(target: AgentLifecycleTarget) {
    return target.installation.availability === "available" && Boolean(target.installation.executablePath);
  }
  function open(kind: AgentCliKind, installationId?: string) {
    lifecycle.open(kind);
    diagnosticInstallationId.value = installationId ?? null;
    if (installationId) void options.inspectHooks(kind);
  }
  function close() { lifecycle.close(); }
  async function refresh(mode: AgentLifecycleVersionRefresh = "cached") {
    await store.refresh(mode);
  }
  function showDiagnostics(installationId: string) {
    diagnosticInstallationId.value = diagnosticInstallationId.value === installationId ? null : installationId;
    if (diagnosticInstallationId.value && lifecycle.agentKind) void options.inspectHooks(lifecycle.agentKind);
  }
  async function savePath(kind: AgentCliKind, path: string) {
    if (savingPath.value || disposed) return;
    const request = ++pathRequest;
    const revision = viewRevision;
    const previousDraft = pathDraft.value;
    const agentLabel = options.agents.value.find((agent) => agent.kind === kind)?.label ?? kind;
    let saved = false;
    let probeRequest: number | null = null;
    savingPath.value = kind;
    delete pathErrors.value[kind];
    try {
      await withTimeout(settings.save({ ...settings.settings, agentCliPaths: { ...settings.settings.agentCliPaths, [kind]: path.trim() } }, settings.settings), 15_000, "保存 Agent 启动路径超时");
      saved = true;
      if (disposed || request !== pathRequest) return;
      if (revision === viewRevision && lifecycle.agentKind === kind && pathDraft.value === previousDraft) pathDraft.value = path.trim();
      const probing = cli.probeCliTools(false);
      probeRequest = cli.cliEnvironmentRequestId;
      activeProbeRequest = probeRequest;
      await withTimeout(probing, 20_000, "重新检测 Agent 启动路径超时");
      probeRequest = null;
      if (disposed || request !== pathRequest) return;
      void overview.refresh(kind, options.workspace.value, true);
      if (lifecycle.agentKind) void store.refresh("ifStale");
      void store.recover();
      Message.success(`已更新 ${agentLabel} 的启动路径`);
    } catch (failure) {
      if (probeRequest !== null) cli.cancelCliToolsProbe(probeRequest);
      if (!disposed && request === pathRequest) {
        const message = `${saved ? "路径已保存，重新检测失败：" : "保存路径失败："}${agentEnvironmentErrorMessage(failure)}`;
        pathErrors.value[kind] = message;
        Message.error(`${agentLabel} · ${message}`);
      }
    } finally {
      if (request === pathRequest) { savingPath.value = null; activeProbeRequest = null; }
    }
  }
  function adopt(target: AgentLifecycleTarget) {
    if (canAdopt(target) && target.installation.executablePath) return savePath(target.agentKind, target.installation.executablePath);
  }
  function adoptCandidate(tool: CliToolProbeResult) {
    if (options.environment.canAdoptDeepScanCandidate(tool)) return savePath(tool.kind, tool.path);
  }
  watch(() => lifecycle.agentKind, (kind) => {
    viewRevision += 1;
    diagnosticInstallationId.value = null;
    options.environment.cancelDeepScan();
    pathDraft.value = kind ? settings.settings.agentCliPaths[kind] ?? "" : "";
  }, { flush: "sync" });
  watch(() => JSON.stringify(options.agents.value.map(({ kind }) => [kind, settings.settings.agentCliPaths[kind]])), () => {
    lifecycle.clearPlan();
    store.invalidate();
  }, { flush: "sync" });
  onScopeDispose(() => {
    disposed = true;
    pathRequest += 1;
    savingPath.value = null;
    if (activeProbeRequest !== null) cli.cancelCliToolsProbe(activeProbeRequest);
    activeProbeRequest = null;
    close();
  });
  return { lifecycle, store, label, pathDraft, pathError, pathErrors, savingPath, selectedPaths, launchPath,
    diagnosticInstallationId, canAdopt, open, close, refresh, showDiagnostics, savePath, adopt, adoptCandidate };
}
