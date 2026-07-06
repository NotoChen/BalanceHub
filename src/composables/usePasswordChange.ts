import { computed, ref } from "vue";
import { Message } from "@arco-design/web-vue";
import type { Provider } from "../stores/providers";

interface UsePasswordChangeOptions {
  providers: { value: Provider[] };
  changePassword: (providerId: string, originalPassword: string, password: string) => Promise<string>;
}

export function usePasswordChange(options: UsePasswordChangeOptions) {
  const passwordChangeVisible = ref(false);
  const passwordChangeProviderId = ref<string | null>(null);
  const passwordChangeLoading = ref(false);

  const passwordChangeProvider = computed(() =>
    options.providers.value.find((provider) => provider.identity.id === passwordChangeProviderId.value) ?? null,
  );

  function openPasswordChange(provider: Provider) {
    passwordChangeProviderId.value = provider.identity.id;
    passwordChangeVisible.value = true;
  }

  async function submitPasswordChange(originalPassword: string, password: string) {
    if (!passwordChangeProvider.value) {
      return;
    }

    passwordChangeLoading.value = true;
    try {
      const message = await options.changePassword(
        passwordChangeProvider.value.identity.id,
        originalPassword,
        password,
      );
      Message.success(message || "密码已更新");
      passwordChangeVisible.value = false;
    } catch (error) {
      Message.error(error instanceof Error ? error.message : String(error));
    } finally {
      passwordChangeLoading.value = false;
    }
  }

  return {
    passwordChangeVisible,
    passwordChangeProvider,
    passwordChangeLoading,
    openPasswordChange,
    submitPasswordChange,
  };
}
