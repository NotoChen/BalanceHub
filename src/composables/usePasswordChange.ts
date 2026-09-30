import { computed, ref, watch } from "vue";
import { Message } from "@arco-design/web-vue";
import type { Provider } from "../stores/providers";
import { useLatestRequest } from "./useLatestRequest";

interface UsePasswordChangeOptions {
  providers: { value: Provider[] };
  changePassword: (providerId: string, originalPassword: string, password: string) => Promise<string>;
}

export function usePasswordChange(options: UsePasswordChangeOptions) {
  const passwordChangeVisible = ref(false);
  const passwordChangeProviderId = ref<string | null>(null);
  const request = useLatestRequest({ timeoutMessage: "修改密码响应超时，请先确认站点上的密码状态，再决定是否重试" });

  const passwordChangeProvider = computed(() =>
    options.providers.value.find((provider) => provider.identity.id === passwordChangeProviderId.value) ?? null,
  );

  function openPasswordChange(provider: Provider) {
    passwordChangeProviderId.value = provider.identity.id;
    passwordChangeVisible.value = true;
  }

  watch([passwordChangeVisible, () => passwordChangeProvider.value?.identity.id], request.invalidate, { flush: "sync" });

  async function submitPasswordChange(originalPassword: string, password: string) {
    const providerId = passwordChangeProvider.value?.identity.id;
    if (!passwordChangeVisible.value || !providerId || request.loading.value) {
      return;
    }

    await request.run(() => options.changePassword(providerId, originalPassword, password), (message) => {
      Message.success(message || "密码已更新");
      passwordChangeVisible.value = false;
    });
  }

  return {
    passwordChangeVisible,
    passwordChangeProvider,
    passwordChangeLoading: request.loading,
    passwordChangeError: request.error,
    openPasswordChange,
    submitPasswordChange,
  };
}
