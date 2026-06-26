import { ref, type Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import { checkInProvider } from "../api/checkin";
import type { Provider } from "../stores/providers";
import { providerCheckedInToday, supportsCheckIn } from "../utils/provider-display";

interface UseCheckInActionsOptions {
  providers: Ref<Provider[]>;
  reload: () => Promise<unknown>;
  notifySystem: (
    title: string,
    body: string,
    options?: { ignoreSwitch?: boolean; provider?: Provider },
  ) => Promise<boolean>;
}

export function useCheckInActions(options: UseCheckInActionsOptions) {
  const checkingInProviderId = ref<string | null>(null);
  const globalCheckInInProgress = ref(false);

  async function checkInProviderAction(provider: Provider) {
    checkingInProviderId.value = provider.identity.id;
    try {
      const result = await checkInProvider(provider.identity.id);
      const message = result.message || (result.ok ? "签到成功" : "签到失败");
      if (result.ok) {
        Message.success(message);
        await options.notifySystem("BalanceHub 签到成功", checkInMarkdown(provider, message), {
          provider,
        });
      } else {
        Message.error(message);
        await options.notifySystem("BalanceHub 签到失败", checkInMarkdown(provider, message), {
          provider,
        });
      }
      await options.reload();
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      Message.error(message);
      await options.notifySystem("BalanceHub 签到异常", checkInMarkdown(provider, message), {
        provider,
      });
      await options.reload();
    } finally {
      checkingInProviderId.value = null;
    }
  }

  async function checkInAllProviders() {
    const targets = options.providers.value.filter(
      (provider) =>
        provider.runtime.enabled && supportsCheckIn(provider) && !providerCheckedInToday(provider),
    );
    if (targets.length === 0) {
      Message.info("没有需要签到的中转站");
      return;
    }

    globalCheckInInProgress.value = true;
    try {
      for (const provider of targets) {
        await checkInProviderAction(provider);
      }
    } finally {
      globalCheckInInProgress.value = false;
    }
  }

  return {
    checkingInProviderId,
    globalCheckInInProgress,
    checkInProviderAction,
    checkInAllProviders,
  };
}

function checkInMarkdown(provider: Provider, message: string) {
  return `**中转站**：${provider.identity.name}\n\n**结果**：${message}`;
}
