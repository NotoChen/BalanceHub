import { onMounted, ref } from "vue";
import { getVersion } from "@tauri-apps/api/app";
import { isWebHost } from "../api/transport.ts";
import { loadHostCapabilities } from "./useHostCapabilities";

export function useAppVersion() {
  const appVersion = ref("");

  onMounted(async () => {
    try {
      // 浏览器版没有 Tauri 版本接口，服务端在能力清单里返回自身版本。
      appVersion.value = isWebHost
        ? (await loadHostCapabilities()).version ?? ""
        : await getVersion();
    } catch {
      appVersion.value = "";
    }
  });

  return {
    appVersion,
  };
}
