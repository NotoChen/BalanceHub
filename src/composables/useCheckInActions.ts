import { computed, ref, type Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import { checkInProvider } from "../api/checkin";
import type { Provider } from "../stores/providers";
import { providerCheckedInToday, supportsCheckIn } from "../utils/provider-actions";

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
  const globalCheckInInProgress = ref(false);

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
      const results = await Promise.all(
        targets.map((provider) => runCheckIn(provider, { reload: false, showMessage: false })),
      );
      await options.reload().catch(() => {});
      const succeeded = results.filter((result) => result === "success").length;
      const failed = results.filter((result) => result === "failed").length;
      const skipped = results.filter((result) => result === "skipped").length;
      const skippedText = skipped > 0 ? `，${skipped} 个正在签到已跳过` : "";
      if (succeeded === 0 && failed === 0 && skipped > 0) {
        Message.info(`并行签到未重复执行：${skipped} 个中转站正在签到`);
      } else if (failed === 0) {
        Message.success(`并行签到完成：${succeeded} 个中转站成功${skippedText}`);
      } else {
        Message.warning(`并行签到完成：${succeeded} 个成功，${failed} 个失败${skippedText}`);
      }
    } finally {
      globalCheckInInProgress.value = false;
    }
  }

  return {
    checkingInProviderIds,
    globalCheckInInProgress,
    checkInProviderAction,
    checkInAllProviders,
  };
}

function checkInMarkdown(provider: Provider, message: string) {
  return `**中转站**：${provider.identity.name}\n\n**结果**：${message}`;
}
