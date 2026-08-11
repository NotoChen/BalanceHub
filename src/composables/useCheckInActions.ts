import { computed, ref, type Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import { checkInProvider } from "../api/checkin";
import type { Provider } from "../stores/providers";

interface UseCheckInActionsOptions {
  providers: Ref<Provider[]>;
  reload: () => Promise<unknown>;
  notifySystem: (
    title: string,
    body: string,
    options?: { ignoreSwitch?: boolean; provider?: Provider },
  ) => Promise<boolean>;
}

type CheckInRunStatus = "success" | "failed" | "skipped";

export function useCheckInActions(options: UseCheckInActionsOptions) {
  const checkingInProviderIdSet = ref<Set<string>>(new Set());
  const checkingInProviderIds = computed(() => [...checkingInProviderIdSet.value]);

  async function runCheckIn(
    provider: Provider,
    behavior: { reload: boolean; showMessage: boolean },
  ): Promise<CheckInRunStatus> {
    const providerId = provider.identity.id;
    if (checkingInProviderIdSet.value.has(providerId)) {
      return "skipped";
    }
    checkingInProviderIdSet.value = new Set(checkingInProviderIdSet.value).add(providerId);
    try {
      const result = await checkInProvider(providerId);
      const message = result.message || (result.ok ? "签到成功" : "签到失败");
      if (result.ok) {
        if (behavior.showMessage) Message.success(message);
        await options.notifySystem("BalanceHub 签到成功", checkInMarkdown(provider, message), {
          provider,
        });
      } else {
        if (behavior.showMessage) Message.error(message);
        await options.notifySystem("BalanceHub 签到失败", checkInMarkdown(provider, message), {
          provider,
        });
      }
      return result.ok ? "success" : "failed";
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      if (behavior.showMessage) Message.error(message);
      await options.notifySystem("BalanceHub 签到异常", checkInMarkdown(provider, message), {
        provider,
      });
      return "failed";
    } finally {
      const next = new Set(checkingInProviderIdSet.value);
      next.delete(providerId);
      checkingInProviderIdSet.value = next;
      if (behavior.reload) {
        await options.reload().catch(() => {});
      }
    }
  }

  async function checkInProviderAction(provider: Provider) {
    await runCheckIn(provider, { reload: true, showMessage: true });
  }

  return {
    checkingInProviderIds,
    checkInProviderAction,
  };
}

function checkInMarkdown(provider: Provider, message: string) {
  return `**中转站**：${provider.identity.name}\n\n**结果**：${message}`;
}
