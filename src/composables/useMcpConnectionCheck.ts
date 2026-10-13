import { computed, onScopeDispose, ref, watch, type Ref } from "vue";
import {
  cancelAgentMcpCheck,
  getAgentMcpCheck,
  startAgentMcpCheck,
  type AgentMcpCheck,
} from "../api/agent-mcp-check";
import { withTimeout } from "../utils/promise-timeout";

export function useMcpConnectionCheck(text: Ref<string>) {
  const result = ref<AgentMcpCheck | null>(null);
  const error = ref("");
  const starting = ref(false);
  const running = computed(
    () => starting.value || Boolean(result.value && !result.value.outcome),
  );
  let generation = 0;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let disposed = false;
  const cancelJob = (id: string) => {
    void withTimeout(cancelAgentMcpCheck(id), 5000, "取消连接检查超时").catch(
      () => {},
    );
  };
  function reset() {
    generation += 1;
    if (timer) clearTimeout(timer);
    timer = null;
    if (result.value && !result.value.outcome) cancelJob(result.value.id);
    starting.value = false;
    result.value = null;
    error.value = "";
  }
  async function poll(current: number, id: string) {
    if (disposed || current !== generation) return;
    try {
      const next = await withTimeout(
        getAgentMcpCheck(id),
        8000,
        "读取连接检查状态超时",
      );
      if (disposed || current !== generation) return;
      result.value = next;
      if (!next.outcome)
        timer = setTimeout(() => {
          void poll(current, id);
        }, 700);
    } catch (e) {
      if (!disposed && current === generation) {
        error.value =
          typeof e === "string"
            ? e
            : e instanceof Error
              ? e.message
              : "读取连接检查失败";
        cancelJob(id);
        result.value = null;
      }
    }
  }
  async function start() {
    reset();
    const current = generation;
    starting.value = true;
    const pending = startAgentMcpCheck(text.value).then((state) => {
      if (disposed || current !== generation) {
        cancelJob(state.id);
        return state;
      }
      result.value = state;
      void poll(current, state.id);
      return state;
    });
    try {
      await withTimeout(pending, 8000, "提交连接检查超时");
    } catch (e) {
      if (!disposed && current === generation) {
        starting.value = false;
        generation += 1;
        error.value =
          typeof e === "string"
            ? e
            : e instanceof Error
              ? e.message
              : "无法开始连接检查";
      }
    } finally {
      if (!disposed && current === generation) starting.value = false;
    }
  }
  function cancel() {
    const previous = result.value;
    reset();
    if (previous)
      result.value = {
        ...previous,
        phase: "completed",
        outcome: "canceled",
        message: "检查已取消",
      };
  }
  watch(text, reset);
  onScopeDispose(() => {
    disposed = true;
    reset();
  });
  return { result, error, running, start, cancel };
}
