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
  /** 后端任务列表里已不存在、但本地仍记为进行中的任务。 */
  const lostOperations = shallowRef<Record<string, T>>({});
  const lost = shallowRef<Record<string, string>>({});
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
    if (lost.value[operation.id]) {
      const messages = { ...lost.value };
      delete messages[operation.id];
      lost.value = messages;
      const lostById = { ...lostOperations.value };
      delete lostById[operation.id];
      lostOperations.value = lostById;
    }
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
        // 恢复流程可能已确认该任务从后端消失；停止轮询，避免继续为它报错。
        if (disposed || isCompleted(id) || !operations.value[id]) return;
        if (Date.now() >= deadline) continue;
        const operation = await withTimeout(options.get(id), Math.min(15_000, Math.max(1, deadline - Date.now())), "读取后台任务超时，请刷新状态");
        if (!disposed && operations.value[id]) accept(operation, id);
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

  /**
   * 读取后端任务列表；返回 true 表示这次结果已被采用。
   * 列表是进程内任务的权威来源：本地仍在进行、列表里却没有的任务只可能是
   * 应用重启或任务被清理后留下的旧快照。继续把它们当成“运行中”会让
   * 会话继续等入口永久被占用、任务中心一直转圈，所以转入“结果未确认”并
   * 解除占用。只核对发起本次读取前已知的任务，避免把读取期间刚提交的
   * 任务误判为丢失。
   */
  async function recover(): Promise<boolean> {
    const request = ++recoveryRequest;
    const knownBefore = new Set(Object.keys(operations.value));
    recovering.value = true;
    recoveryError.value = "";
    try {
      const result = await withTimeout(options.list(), 15_000, "读取后台任务超时");
      if (disposed || request !== recoveryRequest) return false;
      const listed = new Set(result.map((operation) => operation.id));
      for (const operation of result) track(operation);
      const remaining = { ...operations.value };
      let remainingLost = lostOperations.value;
      let remainingMessages = lost.value;
      for (const id of knownBefore) {
        const operation = operations.value[id];
        if (!operation || listed.has(id) || options.isSettled(operation)) continue;
        delete remaining[id];
        polling.value[id] = false;
        delete errors.value[id];
        const message = "后台任务已从任务列表消失（应用可能已重启或任务已过期），结果未确认";
        remainingLost = { ...remainingLost, [id]: operation };
        remainingMessages = { ...remainingMessages, [id]: message };
      }
      operations.value = remaining;
      lostOperations.value = remainingLost;
      lost.value = remainingMessages;
      return true;
    } catch (error) {
      if (!disposed && request === recoveryRequest) recoveryError.value = agentEnvironmentErrorMessage(error);
      return false;
    } finally {
      if (request === recoveryRequest) recovering.value = false;
    }
  }

  function dismissLost(id: string) {
    if (!lost.value[id]) return;
    const messages = { ...lost.value };
    delete messages[id];
    lost.value = messages;
    const operationsById = { ...lostOperations.value };
    delete operationsById[id];
    lostOperations.value = operationsById;
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
  return { operations, polling, errors, canceling, recovering, recoveryError, lost, lostOperations,
    track, recover, cancel, dismissLost };
}
