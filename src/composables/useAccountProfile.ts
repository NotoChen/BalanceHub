import { computed, ref, watch } from "vue";
import { Message } from "@arco-design/web-vue";
import type { Provider, ProviderAccountMutationResult } from "../stores/providers";
import { useLatestRequest } from "./useLatestRequest";

interface UseAccountProfileOptions {
  providers: { value: Provider[] };
  changeProfile: (providerId: string, username: string, displayName: string) => Promise<ProviderAccountMutationResult>;
  redeemCode: (providerId: string, code: string) => Promise<ProviderAccountMutationResult>;
}

export function useAccountProfile(options: UseAccountProfileOptions) {
  const accountProfileVisible = ref(false);
  const accountProfileProviderId = ref<string | null>(null);
  const profileRequest = useLatestRequest({
    timeoutMessage: "保存账号资料超时，请先确认站点上的资料状态，再决定是否重试",
  });
  const redeemRequest = useLatestRequest({
    timeoutMessage: "兑换码请求超时，请先确认站点上的余额状态，再决定是否重试",
  });

  const accountProfileProvider = computed(
    () =>
      options.providers.value.find(
        (provider) => provider.identity.id === accountProfileProviderId.value,
      ) ?? null,
  );

  watch(
    [accountProfileVisible, () => accountProfileProvider.value?.identity.id],
    () => {
      profileRequest.invalidate();
      redeemRequest.invalidate();
    },
    { flush: "sync" },
  );

  function openAccountProfile(provider: Provider) {
    accountProfileProviderId.value = provider.identity.id;
    accountProfileVisible.value = true;
  }

  async function submitProfile(username: string, displayName: string) {
    const providerId = accountProfileProvider.value?.identity.id;
    if (!accountProfileVisible.value || !providerId || profileRequest.loading.value) {
      return;
    }
    if (!username.trim()) {
      Message.error("用户名不能为空");
      return;
    }
    await profileRequest.run(
      () => options.changeProfile(providerId, username.trim(), displayName.trim()),
      (result) => {
        Message.success(result.message || "账号资料已更新");
      },
    );
  }

  async function submitRedeem(code: string) {
    const providerId = accountProfileProvider.value?.identity.id;
    if (!accountProfileVisible.value || !providerId || redeemRequest.loading.value) {
      return;
    }
    if (!code.trim()) {
      Message.error("请输入兑换码");
      return;
    }
    await redeemRequest.run(
      () => options.redeemCode(providerId, code.trim()),
      (result) => {
        Message.success(result.message || "兑换成功");
      },
    );
  }

  return {
    accountProfileVisible,
    accountProfileProvider,
    accountProfileLoading: profileRequest.loading,
    accountProfileError: profileRequest.error,
    redeemLoading: redeemRequest.loading,
    redeemError: redeemRequest.error,
    openAccountProfile,
    submitProfile,
    submitRedeem,
  };
}
