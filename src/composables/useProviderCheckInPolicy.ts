import { ref, watch } from "vue";
import type { ProviderCheckInPolicyPreview, ProviderInput } from "../stores/provider-types";
import { withTimeout } from "../utils/promise-timeout.ts";

interface Options {
  input: () => ProviderInput;
  active: () => boolean;
  preview: (input: ProviderInput) => Promise<ProviderCheckInPolicyPreview>;
  debounceMs?: number;
  timeoutMs?: number;
}

export function useProviderCheckInPolicy(options: Options) {
  const policy = ref<ProviderCheckInPolicyPreview | null>(null);
  const loading = ref(false);
  const error = ref("");
  let revision = 0;

  watch(() => [options.active(), options.input()], (_value, _previous, onCleanup) => {
    const requestId = ++revision;
    policy.value = null;
    error.value = "";
    loading.value = options.active();
    if (!options.active()) return;

    const timer = setTimeout(async () => {
      try {
        const result = await withTimeout(
          options.preview(options.input()), options.timeoutMs ?? 5_000, "读取签到策略超时",
        );
        if (requestId === revision) policy.value = result;
      } catch {
        if (requestId === revision) error.value = "暂时无法读取策略说明，可继续编辑或保存";
      } finally {
        if (requestId === revision) loading.value = false;
      }
    }, options.debounceMs ?? 150);

    onCleanup(() => {
      revision++;
      clearTimeout(timer);
      loading.value = false;
      policy.value = null;
    });
  }, { deep: true, immediate: true });

  return { policy, loading, error };
}
