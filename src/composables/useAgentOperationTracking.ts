import { onScopeDispose, ref, shallowRef } from "vue";
import { withTimeout } from "../utils/promise-timeout";
import { agentEnvironmentErrorMessage } from "../stores/agent-environment";

interface OperationIdentity {
  id: string;
  revision: number;
  canCancel: boolean;
}

/** Store-scoped tracking survives page/modal teardown. It never retries a write. */
export function useAgentOperationTracking<T extends OperationIdentity>(options: {
  get: (id: string) => Promise<T>;
  list: () => Promise<T[]>;
  cancel: (id: string) => Promise<T>;
  identity: (operation: T) => string;
  isSettled: (operation: T) => boolean;
  completed?: (operation: T) => void;
  accepted?: (operation: T) => void;
  timeoutMs: number;
  pollIntervalMs?: (operation: T | undefined) => number;
  pauseAtDeadline?: (operation: T) => boolean;
}) {
  const operations = shallowRef<Record<string, T>>({});
  const polling = ref<Record<string, boolean>>({});
  const errors = ref<Record<string, string>>({});
  const canceling = ref<Record<string, boolean>>({});
  const recovering = ref(false);
  const recoveryError = ref("");
  const completedRevisions = new Map<string, number>();
  let disposed = false;
  let recoveryRequest = 0;
  const isCompleted = (id: string) => Boolean(operations.value[id] && options.isSettled(operations.value[id]));

  function accept(operation: T, expectedId?: string) {
    if (disposed) return operation;
    if (expectedId && operation.id !== expectedId) throw new Error("后台任务返回了不同的目标，请刷新状态");
    const current = operations.value[operation.id];
    if (current && options.identity(current) !== options.identity(operation)) throw new Error("后台任务返回的操作目标不一致，请刷新状态");
    if (current && current.revision >= operation.revision) return current;
    options.accepted?.(operation);
    operations.value = { ...operations.value, [operation.id]: operation };
    delete errors.value[operation.id];
    if (options.isSettled(operation) && completedRevisions.get(operation.id) !== operation.revision) {
      polling.value[operation.id] = false;
      completedRevisions.set(operation.id, operation.revision);
      options.completed?.(operation);
    }
    return operation;
  }

  async function follow(id: string) {
    if (polling.value[id] || disposed || isCompleted(id)) return;
    polling.value[id] = true;
    delete errors.value[id];
    const deadline = Date.now() + options.timeoutMs;
    try {
      while (!disposed && !isCompleted(id)) {
        if (Date.now() >= deadline) {
          const current = operations.value[id];
          if (current && options.pauseAtDeadline?.(current)) return;
          throw new Error("后台任务仍未确认结果，请刷新状态；不会自动重试操作");
        }
        await new Promise<void>((resolve) => globalThis.setTimeout(resolve, options.pollIntervalMs?.(operations.value[id]) ?? 800));
        if (disposed || isCompleted(id)) return;
        if (Date.now() >= deadline) continue;
        const operation = await withTimeout(options.get(id), Math.min(15_000, Math.max(1, deadline - Date.now())), "读取后台任务超时，请刷新状态");
        if (!disposed) accept(operation, id);
      }
    } catch (error) {
      if (!disposed && !isCompleted(id)) errors.value[id] = agentEnvironmentErrorMessage(error);
    } finally {
      polling.value[id] = false;
    }
  }

  function track(operation: T) {
    accept(operation);
    void follow(operation.id);
    return operation;
  }

  async function recover() {
    const request = ++recoveryRequest;
    recovering.value = true;
    recoveryError.value = "";
    try {
      const result = await withTimeout(options.list(), 15_000, "读取后台任务超时");
      if (disposed || request !== recoveryRequest) return;
      for (const operation of result) track(operation);
    } catch (error) {
      if (!disposed && request === recoveryRequest) recoveryError.value = agentEnvironmentErrorMessage(error);
    } finally {
      if (request === recoveryRequest) recovering.value = false;
    }
  }

  async function cancel(id: string) {
    if (!operations.value[id]?.canCancel || canceling.value[id]) return;
    canceling.value[id] = true;
    try {
      const operation = await withTimeout(options.cancel(id), 15_000, "取消请求超时，请刷新任务状态");
      if (!disposed) accept(operation, id);
    } catch (error) {
      if (!disposed) errors.value[id] = agentEnvironmentErrorMessage(error);
    } finally {
      canceling.value[id] = false;
    }
  }

  onScopeDispose(() => { disposed = true; recoveryRequest += 1; });
  return { operations, polling, errors, canceling, recovering, recoveryError, track, recover, cancel };
}
