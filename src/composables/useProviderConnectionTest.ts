import type { Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import type { ProviderConnectionTestResult, ProviderInput } from "../stores/providers";

interface UseProviderConnectionTestOptions {
  draftProvider: ProviderInput;
  editingProviderId: Ref<string | null>;
  testingConnection: Ref<boolean>;
  connectionTestResult: Ref<ProviderConnectionTestResult | null>;
  testProviderConnection: (input: ProviderInput) => Promise<ProviderConnectionTestResult>;
}

export function useProviderConnectionTest(options: UseProviderConnectionTestOptions) {
  async function testConnection() {
    if (!options.draftProvider.identity.baseUrl.trim()) {
      Message.warning("请先填写中转站地址");
      return;
    }

    options.testingConnection.value = true;
    options.connectionTestResult.value = null;
    try {
      const result = await options.testProviderConnection({
        ...options.draftProvider,
        id: options.editingProviderId.value ?? undefined,
      });
      options.connectionTestResult.value = result;
      if (result.ok) {
        Message.success(result.message || "测试通过");
      } else {
        Message.error(result.message || "测试失败");
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      options.connectionTestResult.value = {
        ok: false,
        message,
        available: null,
        used: null,
        quotaDisplay: { quotaDisplayType: "currency", currencySymbol: "$" },
        steps: [],
      };
      Message.error(message);
    } finally {
      options.testingConnection.value = false;
    }
  }

  return { testConnection };
}
