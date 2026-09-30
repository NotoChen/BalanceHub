import { defineStore } from "pinia";
import { onScopeDispose, ref, watch } from "vue";
import { cancelAgentSessionResumeOperation, getAgentSessionResumeOperation, listAgentSessionResumeOperations, resumeAgentSession } from "../api/agent-sessions";
import { useAgentOperationTracking } from "../composables/useAgentOperationTracking";
import { withTimeout } from "../utils/promise-timeout";
import type { AgentSessionResumeOperation, AgentSessionResumeRequest } from "./agent-session-types";
import { useWorkspaceStore } from "./workspaces";

const SUBMIT_TIMEOUT_MESSAGE = "未能确认会话是否已开始继续，请刷新任务状态；不会自动重试";
let requestSequence = 0;

/** Backend operations survive the launch modal and all workspace navigation. */
export const useAgentSessionResumeStore = defineStore("agent-session-resume", () => {
  const workspaces = useWorkspaceStore();
  const starting = ref<Record<string, boolean>>({});
  const startErrors = ref<Record<string, string>>({});
  const startTimes = ref<Record<string, number>>({});
  const unconfirmed = ref<Record<string, string>>({});
  const requestIds = ref<Record<string, string>>({});
  const submittedRequests = new Set<string>();
  const recordedResults = new Set<string>();
  let disposed = false;

  const tracking = useAgentOperationTracking<AgentSessionResumeOperation>({
    get: getAgentSessionResumeOperation,
    list: listAgentSessionResumeOperations,
    cancel: cancelAgentSessionResumeOperation,
    identity: (operation) => operation.sessionRef,
    isSettled: (operation) => operation.state !== "queued" && operation.state !== "running",
    timeoutMs: 3 * 60_000,
    completed(operation) {
      // Recovered old successes must not replace the current directory history.
      if (operation.state === "succeeded" && operation.result && submittedRequests.has(operation.requestId) && !recordedResults.has(operation.id)) {
        recordedResults.add(operation.id);
        workspaces.recordLaunch(operation.result);
      }
    },
  });

  function operationFor(sessionRef: string) {
    return Object.values(tracking.operations.value).filter((operation) => operation.sessionRef === sessionRef)
      .sort((left, right) => Date.parse(right.updatedAt) - Date.parse(left.updatedAt))[0] ?? null;
  }

  function isReserved(sessionRef: string) {
    if (starting.value[sessionRef] || unconfirmed.value[sessionRef]) return true;
    return Object.values(tracking.operations.value).some((operation) => operation.sessionRef === sessionRef
      && (operation.state === "queued" || operation.state === "running" || operation.state === "uncertain"));
  }

  async function submit(input: Omit<AgentSessionResumeRequest, "requestId">) {
    if (disposed || isReserved(input.sessionRef)) return operationFor(input.sessionRef);
    const requestId = `agent-resume-${Date.now()}-${++requestSequence}`;
    const sessionRef = input.sessionRef;
    submittedRequests.add(requestId);
    requestIds.value[sessionRef] = requestId;
    starting.value[sessionRef] = true;
    startTimes.value[sessionRef] = Date.now();
    delete startErrors.value[sessionRef];
    const pending = resumeAgentSession({ ...input, requestId }).then((operation) => {
      if (disposed) return operation;
      if (operation.sessionRef !== sessionRef) throw new Error("后台继续任务返回了不同的会话，请刷新状态");
      delete unconfirmed.value[sessionRef];
      delete startErrors.value[sessionRef];
      // Accept a late acknowledgement in this persistent store; never reopen the modal.
      tracking.track(operation);
      return operation;
    });
    try {
      return await withTimeout(pending, 15_000, SUBMIT_TIMEOUT_MESSAGE);
    } catch (failure) {
      if (!disposed) {
        const message = failure instanceof Error ? failure.message : String(failure);
        startErrors.value[sessionRef] = message;
        if (message === SUBMIT_TIMEOUT_MESSAGE) unconfirmed.value[sessionRef] = requestId;
        void tracking.recover();
      }
      return null;
    } finally {
      starting.value[sessionRef] = false;
    }
  }

  watch(tracking.operations, (operations) => {
    for (const operation of Object.values(operations)) {
      if (requestIds.value[operation.sessionRef] !== operation.requestId) continue;
      delete unconfirmed.value[operation.sessionRef];
      delete startErrors.value[operation.sessionRef];
    }
  }, { flush: "sync" });
  onScopeDispose(() => { disposed = true; });

  return { ...tracking, starting, startErrors, startTimes, unconfirmed, requestIds, submit, operationFor, isReserved };
});
