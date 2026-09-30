import { defineStore } from "pinia";
import { onScopeDispose, ref } from "vue";
import {
  applyAgentConfigurationPlan, cancelAgentConfigurationOperation, getAgentConfigurationOperation, listAgentConfigurationOperations,
} from "../api/agent-configuration";
import { useAgentOperationTracking } from "../composables/useAgentOperationTracking";
import { agentConfigurationErrorDiagnostics, agentConfigurationErrorMessage } from "../utils/agent-configuration-display";
import { withTimeout } from "../utils/promise-timeout";
import { useAgentCatalogStore } from "./agent-catalog";
import { useCliRuntimeStore } from "./cli-runtime";
import { useAgentConfigurationSourcesStore } from "./agent-configuration-sources";
import { useAgentOverviewStore } from "./agent-overview";
import type { AgentConfigurationApplyRequest, AgentConfigurationDiagnostic, AgentConfigurationOperation } from "./agent-configuration-types";
import type { AgentCliKind } from "./provider-types";

export interface AgentConfigurationTaskContext { agentKind: AgentCliKind; workspace: string | null; label: string }
const SUBMIT_TIMEOUT = "尚未确认配置任务是否已提交，请刷新任务状态；不会自动重试写入";

/** Only opaque references and task state live here. Editable text and secret inputs stay in the local editor. */
export const useAgentConfigurationStore = defineStore("agent-configuration", () => {
  const catalog = useAgentCatalogStore();
  const cli = useCliRuntimeStore();
  const sources = useAgentConfigurationSourcesStore();
  const writeHolds = new Map<string, () => void>();
  function releaseWrite(editId: string) { writeHolds.get(editId)?.(); writeHolds.delete(editId); }
  const starting = ref<Record<string, boolean>>({});
  const startErrors = ref<Record<string, string>>({});
  const startDiagnostics = ref<Record<string, AgentConfigurationDiagnostic[]>>({});
  const declined = ref<Record<string, boolean>>({});
  const startTimes = ref<Record<string, number>>({});
  const contexts = ref<Record<string, AgentConfigurationTaskContext>>({});
  const changesRevision = ref(0);
  const ownedEdits = ref<Record<string, boolean>>({});
  const pendingRequests = new Map<string, AgentConfigurationApplyRequest>();
  let disposed = false;

  const tracking = useAgentOperationTracking<AgentConfigurationOperation>({
    get: getAgentConfigurationOperation, list: listAgentConfigurationOperations, cancel: cancelAgentConfigurationOperation,
    identity: (operation) => JSON.stringify([operation.editId, operation.agentKind, operation.sourceIds]),
    isSettled: (operation) => operation.phase === "completed", timeoutMs: 3 * 60_000,
    accepted(operation) {
      if (operation.phase !== "completed" && !writeHolds.has(operation.editId)) writeHolds.set(operation.editId, sources.hold(operation.agentKind));
      ownedEdits.value[operation.editId] = true;
      delete startErrors.value[operation.editId];
      delete startDiagnostics.value[operation.editId];
      delete declined.value[operation.editId];
    },
    completed(operation) {
      sources.invalidate(operation.agentKind);
      releaseWrite(operation.editId);
      if (!operation.files.some((file) => file.state === "applied" || file.state === "unknown")) return;
      changesRevision.value += 1;
      const workspace = contexts.value[operation.editId]?.workspace ?? undefined;
      useAgentOverviewStore().invalidate(operation.agentKind);
      catalog.invalidate(workspace);
      void Promise.allSettled([
        withTimeout(cli.refresh(), 15_000, "刷新默认配置状态超时"),
      ]);
    },
  });

  function operationFor(editId: string) {
    return Object.values(tracking.operations.value).find((operation) => operation.editId === editId) ?? null;
  }
  function ownsEdit(editId: string) { return Boolean(ownedEdits.value[editId]); }

  /** Synchronous handoff before the initiating modal closes or discards its edit. */
  function reserve(request: AgentConfigurationApplyRequest, context: AgentConfigurationTaskContext) {
    if (disposed || ownsEdit(request.editId)) return false;
    ownedEdits.value[request.editId] = true;
    pendingRequests.set(request.editId, { ...request });
    contexts.value[request.editId] = { ...context };
    writeHolds.set(request.editId, sources.hold(context.agentKind));
    starting.value[request.editId] = true;
    startTimes.value[request.editId] = Date.now();
    delete startErrors.value[request.editId];
    return true;
  }

  async function submit(editId: string) {
    const request = pendingRequests.get(editId);
    if (!request || disposed) return null;
    pendingRequests.delete(editId);
    const pending = Promise.resolve().then(() => applyAgentConfigurationPlan(request)).then((operation) => {
      if (disposed) return operation;
      if (operation.editId !== editId || operation.agentKind !== contexts.value[editId]?.agentKind) throw new Error("后台任务目标不一致");
      tracking.track(operation); // Late acknowledgements are accepted by this store, never by the closed modal.
      return operation;
    });
    try {
      return await withTimeout(pending, 15_000, SUBMIT_TIMEOUT);
    } catch (error) {
      if (!disposed) {
        startErrors.value[editId] = agentConfigurationErrorMessage(error, SUBMIT_TIMEOUT);
        startDiagnostics.value[editId] = agentConfigurationErrorDiagnostics(error);
        declined.value[editId] = Boolean(error && typeof error === "object" && "kind" in error && (error.kind === "planExpired" || error.kind === "editExpired"));
        if (declined.value[editId]) releaseWrite(editId);
        void tracking.recover();
      }
      return null;
    } finally {
      starting.value[editId] = false;
    }
  }

  onScopeDispose(() => { disposed = true; pendingRequests.clear(); for (const release of writeHolds.values()) release(); writeHolds.clear(); });
  return { ...tracking, starting, startErrors, startDiagnostics, declined, startTimes, contexts, changesRevision, reserve, submit, ownsEdit, operationFor };
});
