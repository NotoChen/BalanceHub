import { acceptHMRUpdate, defineStore } from "pinia";
import { computed, ref, shallowRef } from "vue";
import {
  getWslInventory,
  startWslDistribution,
  type WslInventory,
} from "../api/agent-wsl";
import { withTimeout } from "../utils/promise-timeout";
import { useHostPlatform } from "../composables/useHostPlatform";

export const useAgentWslStore = defineStore("agent-wsl", () => {
  const host = useHostPlatform();
  const supported = computed(() =>
    host.platform.value === null ? null : host.platform.value === "windows",
  );
  const inventory = shallowRef<WslInventory | null>(null);
  const loading = ref(false);
  const starting = ref("");
  const error = ref("");
  let pending: Promise<void> | null = null;
  void host.ensure();

  function run(action: () => Promise<WslInventory>, startId = "") {
    if (pending) return pending;
    if (supported.value === false) return Promise.resolve();
    loading.value = true;
    starting.value = startId;
    error.value = "";
    pending = withTimeout(
      host
        .ensure()
        .then((platform) => (platform === "windows" ? action() : null)),
      50_000,
      "WSL 检测超时，请刷新核对状态",
    )
      .then((value) => {
        if (!value) return;
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
    return run(() => startWslDistribution(id), id);
  }
  return {
    supported,
    inventory,
    loading,
    starting,
    error,
    refresh,
    ensure,
    start,
  };
});

if (import.meta.hot) {
  // Keep the inventory while updating platform guards in the running dev app.
  import.meta.hot.accept(acceptHMRUpdate(useAgentWslStore, import.meta.hot));
}
