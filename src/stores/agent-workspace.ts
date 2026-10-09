import { acceptHMRUpdate, defineStore } from "pinia";
import { computed, ref } from "vue";
import type { AgentCliKind } from "./provider-types";
import type { AgentSessionRoleFilter } from "./agent-session-types";

export type AppWorkspaceView = "providers" | "agents";
export type AgentWorkspacePage = "overview" | "library" | "skill" | "mcp" | "extension" | "hook" | "sessions";
export type AgentSessionView = "active" | "history";
export interface AgentOperationSelection { kind: "catalog" | "lifecycle" | "native"; id: string }
type AgentSearchScope = Exclude<AgentWorkspacePage, "sessions"> | `sessions:${AgentSessionView}`;

export const agentWorkspacePages: { value: AgentWorkspacePage; label: string; searchPlaceholder: string }[] = [
  { value: "overview", label: "总览", searchPlaceholder: "搜索 Agent" },
  { value: "library", label: "共享库", searchPlaceholder: "搜索共享资产名称或摘要" },
  { value: "skill", label: "Skill", searchPlaceholder: "搜索 Skill 名称或来源" },
  { value: "mcp", label: "MCP", searchPlaceholder: "搜索 MCP 名称或来源" },
  { value: "extension", label: "插件", searchPlaceholder: "搜索插件名称或来源" },
  { value: "hook", label: "Hook", searchPlaceholder: "搜索 Hook 名称或来源" },
  { value: "sessions", label: "会话", searchPlaceholder: "搜索会话标题、ID 或正文" },
];

/** Navigation only. Native identities, capabilities and operations remain in their domain stores. */
export const useAgentWorkspaceStore = defineStore("agent-workspace", () => {
  const view = ref<AppWorkspaceView>("providers");
  const page = ref<AgentWorkspacePage>("overview");
  const sessionView = ref<AgentSessionView>("history");
  const sessionWorkspaceMode = ref<"home" | "all">("all");
  const sessionWorkspaceSelection = ref<string | null>(null);
  const sessionRoleFilter = ref<AgentSessionRoleFilter>("all");
  const sessionAgentFilter = ref<AgentCliKind | null>(null);
  const agentFilter = ref<AgentCliKind | null>(null);
  const queries = ref<Partial<Record<AgentSearchScope, string>>>({});
  const searchScope = computed<AgentSearchScope>(() => page.value === "sessions" ? `sessions:${sessionView.value}` : page.value);
  const query = computed({
    get: () => queries.value[searchScope.value] ?? "",
    set: (value: string) => { queries.value[searchScope.value] = value; },
  });
  const sessionHistoryQuery = computed({
    get: () => queries.value["sessions:history"] ?? "",
    set: (value: string) => { queries.value["sessions:history"] = value; },
  });
  const searchPlaceholder = computed(() => page.value === "sessions" && sessionView.value === "active"
    ? "搜索活动会话、目录或模型"
    : agentWorkspacePages.find((item) => item.value === page.value)?.searchPlaceholder ?? "搜索 Agent");
  const workspacePath = ref<string | undefined>();
  const navigationRevision = ref(0);
  const operationDetails = ref<AgentOperationSelection | null>(null);
  const libraryAssetId = ref<string | null>(null);

  function openOperation(kind: AgentOperationSelection["kind"], id: string) {
    setView("agents");
    operationDetails.value = { kind, id };
  }
  function closeOperation() { operationDetails.value = null; }

  function setView(next: AppWorkspaceView) {
    if (view.value === next) return;
    view.value = next;
    navigationRevision.value += 1;
  }

  function navigate(next: AgentWorkspacePage, agent?: AgentCliKind | null) {
    closeOperation();
    libraryAssetId.value = null;
    if (agent !== undefined) agentFilter.value = agent;
    page.value = next;
    view.value = "agents";
    navigationRevision.value += 1;
  }

  function openPage(next: AgentWorkspacePage, agent?: AgentCliKind | null) {
    if (next === "sessions") { openSessions("history", agent); return; }
    if (next === "library") { openLibrary(); return; }
    navigate(next, agent);
  }

  function openLibrary(assetId: string | null = null) {
    queries.value.library = "";
    navigate("library", null);
    libraryAssetId.value = assetId;
  }

  function openSessions(view: AgentSessionView = "history", agent?: AgentCliKind | null, options: { workspaceMode?: "home" | "all" } = {}) {
    if (agent !== undefined) sessionAgentFilter.value = agent;
    if (options.workspaceMode) {
      sessionWorkspaceMode.value = options.workspaceMode;
      if (options.workspaceMode === "home") sessionWorkspaceSelection.value = null;
    }
    sessionView.value = view;
    navigate("sessions", sessionAgentFilter.value);
  }

  function selectSessionView(next: AgentSessionView) {
    if (sessionView.value === next) return;
    sessionView.value = next;
    navigationRevision.value += 1;
  }

  function selectAgent(agent: AgentCliKind | null) {
    if (page.value === "sessions") sessionAgentFilter.value = agent;
    if (agentFilter.value === agent) return;
    agentFilter.value = agent;
    navigationRevision.value += 1;
  }

  function selectWorkspace(path?: string) {
    const next = path?.trim() || undefined;
    if (workspacePath.value === next) return;
    workspacePath.value = next;
    navigationRevision.value += 1;
  }

  return { view, page, sessionView, sessionWorkspaceMode, sessionWorkspaceSelection, sessionRoleFilter, sessionAgentFilter, agentFilter, queries, query, sessionHistoryQuery, searchPlaceholder, workspacePath, navigationRevision,
    operationDetails, libraryAssetId, openOperation, closeOperation, setView, openPage, openLibrary, openSessions, selectSessionView, selectAgent, selectWorkspace };
});

if (import.meta.hot) {
  // Update actions as well as views while preserving the current navigation state.
  import.meta.hot.accept(acceptHMRUpdate(useAgentWorkspaceStore, import.meta.hot));
}
