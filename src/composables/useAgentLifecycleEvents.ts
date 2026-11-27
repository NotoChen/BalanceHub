import { onMounted, onScopeDispose } from "vue";
import { listen, type UnlistenFn } from "../api/transport.ts";
import { useAgentLifecycleStore } from "../stores/agent-lifecycle";
import type { AgentLifecycleCatalog, AgentLifecycleOperation } from "../stores/agent-lifecycle-types";
import { withTimeout } from "../utils/promise-timeout";
import { runWhenIdle } from "../utils/idle";

/** The App observes upgrades even when the Agent workspace has never been opened. */
export function useAgentLifecycleEvents() {
  const store = useAgentLifecycleStore();
  const unlisteners = new Map<string, UnlistenFn>();
  let disposed = false;
  let retry: ReturnType<typeof setTimeout> | undefined;
  async function subscribe<T>(name: string, receive: (value: T) => void) {
    if (unlisteners.has(name)) return;
    let expired = false;
    try {
      const listener = await withTimeout(listen<T>(name, ({ payload }) => { if (!disposed) receive(payload); }).then((stop) => {
        if (disposed || expired) stop();
        return stop;
      }), 10_000, "更新状态连接超时");
      if (disposed) listener(); else unlisteners.set(name, listener);
    } finally { expired = true; }
  }
  async function connect(initial = false) {
    const results = await Promise.allSettled([
      subscribe<AgentLifecycleCatalog>("agent-lifecycle-catalog-updated", (value) => store.acceptCatalog(value)),
      subscribe<AgentLifecycleOperation>("agent-lifecycle-operation-updated", (value) => { if (value.origin === "automatic") store.track(value); }),
      // 事件通道断开期间的通知会丢失；恢复后重新读取一次，避免升级状态
      // 和自动更新策略长期停在断线前的快照上。
      subscribe("balancehub-events-reconnected", () => {
        void store.recover();
        void store.refresh("cached");
        void connect();
      }),
    ]);
    if (!disposed) {
      const failed = results.some((result) => result.status === "rejected");
      store.eventError = failed ? "更新状态连接暂时中断，正在重新连接；后台任务仍会继续" : "";
      if (failed) retry = setTimeout(() => { void connect(); }, 10_000);
      // 首轮目录读取和任务恢复会扫描安装目录并核对升级任务；订阅先就位，
      // 这些读取等首屏空闲后再做。重连补偿保持立即执行。
      const sync = () => {
        void store.recover();
        void store.refresh("cached");
      };
      if (initial) runWhenIdle(sync);
      else sync();
    }
  }
  onMounted(() => { void connect(true); });
  onScopeDispose(() => { disposed = true; clearTimeout(retry); for (const unlisten of unlisteners.values()) unlisten(); });
}
