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
  let fetchedAt = 0;
  // 面板每次激活都重新检测会让界面等一次 WSL 扫描；一直用首次结果又会把
  // 新装或已停止的发行版永远停在旧状态。保留旧列表，超过后端缓存时间再
  // 后台刷新，是这两者之间唯一不阻塞打开的折中。
  const FRESH_MS = 30_000;
  void host.ensure();

  function run(
    action: () => Promise<WslInventory>,
    startId = "",
    silent = false,
  ) {
    if (pending) return pending;
    if (supported.value === false) return Promise.resolve();
    if (!silent) {
      loading.value = true;
      starting.value = startId;
      error.value = "";
    }
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
        fetchedAt = Date.now();
        error.value = value.error ?? "";
      })
      .catch((failure) => {
        error.value =
          failure instanceof Error ? failure.message : String(failure);
      })
      .finally(() => {
        if (!silent) {
          loading.value = false;
          starting.value = "";
        }
        pending = null;
      });
    return pending;
  }
  function refresh(force = true) {
    return run(() => getWslInventory(force));
  }
  function ensure() {
    if (!inventory.value) return refresh(false);
    if (Date.now() - fetchedAt < FRESH_MS) return Promise.resolve();
    // 激活不等待这次刷新：旧列表已经可读，结果回来后原地替换。
    void run(() => getWslInventory(false), "", true);
    return Promise.resolve();
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
