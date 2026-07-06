import { onMounted, ref } from "vue";
import { getVersion } from "@tauri-apps/api/app";

export function useAppVersion() {
  const appVersion = ref("");

  onMounted(async () => {
    try {
      appVersion.value = await getVersion();
    } catch {
      appVersion.value = "";
    }
  });

  return {
    appVersion,
  };
}
