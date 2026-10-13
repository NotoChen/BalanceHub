import { defineStore } from "pinia";
import { ref, shallowRef } from "vue";
import {
  getWslInventory,
  startWslDistribution,
  type WslInventory,
} from "../api/agent-wsl";
import { withTimeout } from "../utils/promise-timeout";

export const useAgentWslStore = defineStore("agent-wsl", () => {
  const inventory = shallowRef<WslInventory | null>(null);
  const loading = ref(false);
  const starting = ref("");
  const error = ref("");
  let pending: Promise<void> | null = null;
  function run(action: () => Promise<WslInventory>) {
    if (pending) return pending;
    loading.value = true;
    error.value = "";
    pending = withTimeout(action(), 50_000, "WSL 检测超时，请刷新核对状态")
      .then((value) => {
        inventory.value = value;
        error.value = value.error ?? "";
      })
      .catch((failure) => {
        error.value =
          failure instanceof Error ? failure.message : String(failure);
      })
      .finally(() => {
        loading.value = false;
        starting.value = "";
        pending = null;
      });
    return pending;
  }
  function refresh(force = true) {
    return run(() => getWslInventory(force));
  }
  function ensure() {
    return inventory.value ? Promise.resolve() : refresh(false);
  }
  function start(id: string) {
    if (pending) return pending;
    starting.value = id;
    return run(() => startWslDistribution(id));
  }
  return { inventory, loading, starting, error, refresh, ensure, start };
});
