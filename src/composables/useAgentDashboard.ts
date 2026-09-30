import { computed, onMounted, onScopeDispose, reactive, ref, watch, type Ref } from "vue";
import { storeToRefs } from "pinia";
import { Message } from "@arco-design/web-vue";
import { openAgentDocumentation, type AgentDocumentationPage } from "../api/agent-documentation";
import { useAgentWorkspaceStore } from "../stores/agent-workspace";
import { useSettingsStore } from "../stores/settings";
import { useWorkspaceStore } from "../stores/workspaces";
import { useCliRuntimeStore } from "../stores/cli-runtime";
import { useAgentCatalogStore } from "../stores/agent-catalog";
import { useAgentLifecycleStore } from "../stores/agent-lifecycle";
import { useAgentOverviewStore } from "../stores/agent-overview";
import { useAgentCardOrderStore } from "../stores/agent-card-order";
import { useAgentConfigurationSourcesStore } from "../stores/agent-configuration-sources";
import { agentEnvironmentKey, useAgentEnvironmentStore } from "../stores/agent-environment";
import { useAgentEnvironmentCenter } from "./useAgentEnvironmentCenter";
import { useAgentHookConsole } from "./useAgentHookConsole";
import { useAgentCatalogConsole } from "./useAgentCatalogConsole";
import { useAgentResourceContent } from "./useAgentResourceContent";
import { useAgentInstallationManager } from "./useAgentInstallationManager";
import { useAgentVersionChecks } from "./useAgentVersionChecks";
import { useAgentHistorySummary } from "./useAgentHistorySummary";
import { useAgentConfiguration } from "./useAgentConfiguration";
import { activeAgentRuntimeSessions, isConfirmedAgentRuntimeSession } from "../utils/agent-runtime";
import { availableCliKinds } from "../utils/cli-environment";
import { withTimeout } from "../utils/promise-timeout";
import type { AgentCliKind, AgentRuntimeSnapshot, CliRuntimeSnapshot } from "../stores/provider-types";

export function useAgentDashboard(options: { active: Ref<boolean>; cliRuntime: Ref<CliRuntimeSnapshot>; runtime: Ref<AgentRuntimeSnapshot> }) {
  const navigation = useAgentWorkspaceStore();
  const { workspacePath, navigationRevision } = storeToRefs(navigation);
  const settings = useSettingsStore();
  const workspaces = useWorkspaceStore();
  const kinds = computed(() => options.cliRuntime.value.agents.map((agent) => agent.kind));
  const historyActive = computed(() => options.active.value && navigation.page === "overview");
  const historyScopeKey = computed(() => JSON.stringify([...new Set(workspaces.workspaces.map((workspace) => workspace.path))].sort()));
  const history = reactive(useAgentHistorySummary(historyActive, historyScopeKey, kinds));
  const cli = useCliRuntimeStore();
  const catalogStore = useAgentCatalogStore();
  const lifecycleStore = useAgentLifecycleStore();
  const overview = useAgentOverviewStore();
  const cardOrder = useAgentCardOrderStore();
  const orderedAgents = computed(() => cardOrder.sorted(options.cliRuntime.value.agents));
  const nativeStore = useAgentEnvironmentStore();
  const configurationSources = useAgentConfigurationSourcesStore();
  const pathSettings = reactive({ ...settings.settings, agentCliPaths: { ...settings.settings.agentCliPaths } });
  watch(() => settings.settings.agentCliPaths, (paths) => { pathSettings.agentCliPaths = { ...paths }; });
  const environment = reactive(useAgentEnvironmentCenter({ settings: pathSettings }));
  environment.selectWorkspace(workspacePath.value);
  const hooks = reactive(useAgentHookConsole());
  const catalog = reactive(useAgentCatalogConsole({ workspace: workspacePath, navigationRevision }));
  const resource = reactive(useAgentResourceContent({ catalog, workspace: workspacePath }));
  const installation = reactive(useAgentInstallationManager({
    agents: computed(() => options.cliRuntime.value.agents), workspace: workspacePath,
    navigationRevision, environment, inspectHooks: hooks.inspect,
  }));
  const selectedAgentKind = ref<AgentCliKind | null>(null);
  const configuration = reactive(useAgentConfiguration({ agentKind: selectedAgentKind, workspace: workspacePath }));
  const launchKind = ref<AgentCliKind | null>(null);
  const focusedHookId = ref<string | null>(null);
  let disposed = false;
  const workspaceOptions = computed(() => [
    { value: "", label: "全局配置" },
    ...workspaces.workspaces.map((workspace) => ({ value: workspace.path, label: workspace.path.split(/[\\/]/).filter(Boolean).slice(-1)[0] || workspace.path })),
  ]);
  const catalogLoading = computed(() => Boolean(catalogStore.loading[agentEnvironmentKey(workspacePath.value)]));
  const loading = computed(() => navigation.page === "overview"
    ? (!catalog.catalog && catalogLoading.value) || kinds.value.some((kind) => !overview.get(kind, workspacePath.value) && overview.refreshing[overview.key(kind, workspacePath.value)])
    : catalogLoading.value);
  const installations = computed(() => catalog.catalog?.inventory.installations ?? environment.inventory?.installations ?? []);
  const error = computed(() => (navigation.page === "overview"
    ? kinds.value.map((kind) => overview.errors[overview.key(kind, workspacePath.value)]).find(Boolean) : "")
    || catalogStore.loadErrors[agentEnvironmentKey(workspacePath.value)] || "");
  const activeSessions = computed(() => activeAgentRuntimeSessions(options.runtime.value));
  const runtimeSessions = computed(() => {
    const needle = navigation.query.trim().toLocaleLowerCase();
    return activeSessions.value.filter((session) => (!navigation.agentFilter || session.agentKind === navigation.agentFilter)
      && (!needle || [session.title, session.workdir, session.model, session.provider?.providerName,
        options.cliRuntime.value.agents.find((agent) => agent.kind === session.agentKind)?.label]
        .some((value) => value?.toLocaleLowerCase().includes(needle))));
  });
  const cards = computed(() => {
    const needle = navigation.query.trim().toLocaleLowerCase();
    return orderedAgents.value.filter((agent) => (!navigation.agentFilter || agent.kind === navigation.agentFilter)
      && (!needle || `${agent.label} ${agent.kind}`.toLocaleLowerCase().includes(needle))).map((agent) => {
      const saved = overview.get(agent.kind, workspacePath.value);
      const probe = saved?.probe ?? cli.cliEnvironmentProbe?.tools.find((tool) => tool.kind === agent.kind);
      const refreshing = Boolean(overview.refreshing[overview.key(agent.kind, workspacePath.value)]);
      return {
      agent,
      inventoryReady: Boolean(catalog.catalog),
      installations: installations.value.filter((installation) => installation.agentKind === agent.kind),
      lifecycleTargets: lifecycleStore.catalog?.targets.filter((target) => target.agentKind === agent.kind) ?? [],
      versionChecking: lifecycleStore.checkingVersions,
      versionCheckError: lifecycleStore.error,
      selectedPath: probe?.path || settings.settings.agentCliPaths[agent.kind] || null,
      selectedVersion: probe?.version || null,
      counts: catalog.catalog?.counts[agent.kind] ?? { skill: null, mcp: null, extension: null },
      hookCount: catalog.catalog?.inventory.hookRuleCounts.find((entry) => entry.agentKind === agent.kind)?.ruleCount ?? null,
      historyCount: history.agentCounts.find((entry) => entry.agentKind === agent.kind) ?? null,
      historyLoading: history.isLoading(agent.kind),
      historyError: history.errorFor(agent.kind),
      refreshing: Boolean(overview.rescanning[overview.key(agent.kind, workspacePath.value)]) || catalogLoading.value,
      runningCount: activeSessions.value.filter((session) => session.agentKind === agent.kind && isConfirmedAgentRuntimeSession(session)).length,
      canLaunch: probe ? availableCliKinds({ tools: [probe] }, "temporaryLaunch").includes(agent.kind) : false,
      configurationSnapshot: saved?.configuration ?? null,
      configurationLoading: !saved?.configuration && refreshing,
      configurationError: saved?.configurationError ?? "",
      configurationStale: Boolean(saved?.configurationError),
    }; });
  });
  async function refreshAssets(force = false) {
    const workspace = workspacePath.value;
    if (force || catalogStore.stale[agentEnvironmentKey(workspace)] || !catalogStore.catalogs[agentEnvironmentKey(workspace)]) await catalogStore.refresh(workspace, force);
  }
  function reorderCards(ids: string[]) {
    return cardOrder.reorder(orderedAgents.value.map((agent) => agent.kind), ids);
  }
  async function refresh() {
    await Promise.allSettled([
      ...(navigation.page === "overview" || navigation.page === "sessions" ? [overview.check(kinds.value, workspacePath.value, true)] : []),
      ...(navigation.page !== "sessions" ? [refreshAssets(true)] : []),
      ...(historyActive.value ? [history.refresh()] : []),
    ]);
  }
  async function refreshCard(kind: AgentCliKind) {
    await Promise.allSettled([overview.refresh(kind, workspacePath.value, true), ...(historyActive.value ? [history.refresh(kind)] : [])]);
  }
  function openConfigurationFile(kind: AgentCliKind, sourceId: string) {
    selectedAgentKind.value = kind;
    const source = overview.get(kind, workspacePath.value)?.configuration?.sources.find((item) => item.sourceId === sourceId);
    configuration.openFile(sourceId, source);
  }
  async function openDocumentation(kind: AgentCliKind, page: AgentDocumentationPage = "configuration") {
    try { await withTimeout(openAgentDocumentation(kind, page), 10_000, "打开官方文档超时"); }
    catch { Message.error("无法打开官方文档，请稍后重试"); }
  }
  function openNative(id: string) {
    const asset = catalog.catalog?.assets.find((item) => item.bindings.some((binding) => binding.native.stableId === id));
    const binding = asset?.bindings.find((item) => item.native.stableId === id);
    if (asset && binding) {
      environment.assets.closeDetail();
      catalog.openDetail(asset.id, `binding:${binding.id}`);
      return;
    }
    catalog.closeDetail();
    environment.assets.openDetail(id);
  }
  function showHook(id: string) {
    const asset = environment.assets.catalog.indexes.assets.get(id);
    if (asset?.category !== "hook") return;
    navigation.openPage("hook", asset.agentKind);
    focusedHookId.value = id;
  }
  async function confirmHookPlan() {
    if (!hooks.pendingPlan || !hooks.canApplyPlan) return;
    await hooks.confirmPlan();
    if (!disposed) await catalogStore.refresh(workspacePath.value, true);
  }
  function invalidate() {
    installation.close();
    configuration.invalidate();
    navigation.closeOperation();
    selectedAgentKind.value = null;
    launchKind.value = null;
    focusedHookId.value = null;
    environment.assets.invalidateTransient();
    hooks.closePlan();
  }
  watch(navigationRevision, invalidate, { flush: "sync" });
  watch(workspacePath, () => environment.selectWorkspace(workspacePath.value));
  function checkOverview() {
    if (!disposed && options.active.value && navigation.page === "overview" && document.visibilityState !== "hidden") {
      void overview.check(kinds.value, workspacePath.value);
    }
  }
  watch(() => overview.invalidationRevision, checkOverview);
  watch(() => [options.active.value, navigation.page, workspacePath.value, kinds.value.join("|")] as const, () => {
    if (!options.active.value) return;
    if (navigation.page === "overview") checkOverview();
    else if (navigation.page === "sessions") void overview.check(kinds.value, workspacePath.value);
    if (navigation.page !== "sessions") void refreshAssets();
  }, { immediate: true });
  watch(() => kinds.value.map((kind) => [kind, configurationSources.get(kind, workspacePath.value).value?.revision] as const), (current, previous) => {
    for (const [kind, configurationRevision] of current) {
      const before = previous?.find((entry) => entry[0] === kind);
      if (configurationRevision && configurationRevision !== before?.[1]) {
        void overview.refresh(kind, workspacePath.value);
        checkCatalogSources();
      }
    }
  });
  const versionCheckActive = computed(() => options.active.value && kinds.value.length > 0 && navigation.page === "overview"
    && !kinds.value.some((kind) => overview.refreshing[overview.key(kind, workspacePath.value)]));
  const localVersionRevision = computed(() => JSON.stringify(kinds.value.map((kind) => {
    const saved = overview.get(kind, workspacePath.value);
    return [kind, settings.settings.agentCliPaths[kind], saved?.probe.path, saved?.probe.version,
      installations.value.filter((installation) => installation.agentKind === kind).map((installation) => [installation.id, installation.executableRevision, installation.installedVersion])];
  })));
  useAgentVersionChecks(versionCheckActive, localVersionRevision);
  watch(() => settings.settings.agentCliPaths, (paths, previous) => {
    for (const kind of kinds.value) if (paths[kind] !== previous?.[kind]) void overview.invalidate(kind);
    catalogStore.invalidate(workspacePath.value);
    if (options.active.value) void refreshAssets();
  });
  function checkCatalogSources() {
    if (disposed || !options.active.value || navigation.page === "sessions" || document.visibilityState === "hidden") return;
    const workspace = workspacePath.value;
    const key = agentEnvironmentKey(workspace);
    void catalogStore.check(workspace).then(() => {
      if (!disposed && options.active.value && workspacePath.value === workspace && navigation.page !== "sessions"
        && document.visibilityState !== "hidden" && catalogStore.stale[key] && !catalogStore.loading[key]) {
        void catalog.refresh(workspace);
      }
    });
  }
  watch(() => catalogStore.stale[agentEnvironmentKey(workspacePath.value)], (stale) => {
    if (stale) checkCatalogSources();
  });
  function checkActiveSources() {
    if (disposed || !options.active.value || document.visibilityState === "hidden") return;
    checkOverview();
    checkCatalogSources();
  }
  watch([options.active, workspacePath], checkActiveSources);
  const sourceTimer = globalThis.setInterval(checkActiveSources, 30_000);
  globalThis.addEventListener("focus", checkActiveSources);
  document.addEventListener("visibilitychange", checkActiveSources);
  let nativeRefreshTimer: ReturnType<typeof globalThis.setTimeout> | null = null;
  watch(() => Object.values(nativeStore.operations).filter((operation) => operation.phase === "completed" && !catalogStore.nativeOperationIds.has(operation.id)).map((operation) => `${operation.id}:${operation.revision}`).join("|"), (completed) => {
    if (!completed) return;
    if (nativeRefreshTimer !== null) globalThis.clearTimeout(nativeRefreshTimer);
    nativeRefreshTimer = globalThis.setTimeout(() => {
      nativeRefreshTimer = null;
      checkOverview();
      catalogStore.invalidate(workspacePath.value);
      checkCatalogSources();
    }, 100);
  });
  onMounted(() => {
    void catalogStore.recover();
    void lifecycleStore.recover();
    void environment.assets.recoverOperations();
  });
  onScopeDispose(() => {
    disposed = true;
    globalThis.clearInterval(sourceTimer);
    globalThis.removeEventListener("focus", checkActiveSources);
    document.removeEventListener("visibilitychange", checkActiveSources);
    if (nativeRefreshTimer !== null) globalThis.clearTimeout(nativeRefreshTimer);
  });
  return { navigation, environment, configuration, hooks, catalog, resource, catalogStore, installation, lifecycleStore,
    launchKind, focusedHookId, workspaceOptions, loading, error, cards, runtimeSessions, installations, refresh, refreshCard, reorderCards,
    openConfigurationFile, openDocumentation, openNative, showHook, confirmHookPlan, invalidate };
}
