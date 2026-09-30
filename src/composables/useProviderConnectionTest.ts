import { watch, type Ref } from "vue";
import { useLatestRequest } from "./useLatestRequest.ts";
import type { ProviderConnectionTestResult, ProviderInput } from "../stores/providers";

interface UseProviderConnectionTestOptions {
  draftProvider: ProviderInput;
  drawerVisible: Ref<boolean>;
  editorSession: Ref<number>;
  editingProviderId: Ref<string | null>;
  connectionTestResult: Ref<ProviderConnectionTestResult | null>;
  testProviderConnection: (input: ProviderInput) => Promise<ProviderConnectionTestResult>;
}

export function useProviderConnectionTest(options: UseProviderConnectionTestOptions) {
  const request = useLatestRequest({ timeoutMs: 60_000, timeoutMessage: "连接测试超时，请检查地址、网络或认证信息后重试" });
  watch([
    options.drawerVisible,
    options.editorSession,
    () => JSON.stringify(snapshotInput(options.draftProvider, options.editingProviderId.value)),
  ], () => {
    request.invalidate();
    options.connectionTestResult.value = null;
  }, { flush: "sync" });

  watch(request.error, (message) => {
    if (message) options.connectionTestResult.value = failedResult(message);
  }, { flush: "sync" });

  async function testConnection() {
    if (!options.drawerVisible.value || request.loading.value) return;
    if (!options.draftProvider.identity.baseUrl.trim()) {
      options.connectionTestResult.value = failedResult("请先填写中转站地址");
      return;
    }
    options.connectionTestResult.value = null;
    const input = snapshotInput(options.draftProvider, options.editingProviderId.value);
    await request.run(() => options.testProviderConnection(input), (result) => {
      options.connectionTestResult.value = result;
    });
  }

  return { testConnection, testingConnection: request.loading };
}

function failedResult(message: string): ProviderConnectionTestResult {
  return { ok: false, message, available: null, used: null, quotaDisplay: { quotaDisplayType: "currency", currencySymbol: "$" }, steps: [] };
}

function snapshotInput(draftProvider: ProviderInput, providerId: string | null): ProviderInput {
  return JSON.parse(
    JSON.stringify({
      ...draftProvider,
      id: providerId ?? undefined,
    }),
  ) as ProviderInput;
}
