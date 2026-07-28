import type { Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import type { ProviderConnectionTestResult, ProviderInput } from "../stores/providers";

interface UseProviderConnectionTestOptions {
  draftProvider: ProviderInput;
  drawerVisible: Ref<boolean>;
  editorSession: Ref<number>;
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
    const editorSession = options.editorSession.value;
    const providerId = options.editingProviderId.value;
    const input = snapshotInput(options.draftProvider, providerId);
    const inputFingerprint = JSON.stringify(input);
    const requestIsCurrent = () =>
      options.drawerVisible.value &&
      options.editorSession.value === editorSession &&
      options.editingProviderId.value === providerId &&
      JSON.stringify(snapshotInput(options.draftProvider, providerId)) === inputFingerprint;
    try {
      const result = await options.testProviderConnection(input);
      if (!requestIsCurrent()) return;
      options.connectionTestResult.value = result;
      if (result.ok) {
        Message.success(result.message || "测试通过");
      } else {
        Message.error(result.message || "测试失败");
      }
    } catch (error) {
      if (!requestIsCurrent()) return;
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
      if (
        options.drawerVisible.value &&
        options.editorSession.value === editorSession
      ) {
        options.testingConnection.value = false;
      }
    }
  }

  return { testConnection };
}

function snapshotInput(draftProvider: ProviderInput, providerId: string | null): ProviderInput {
  return JSON.parse(
    JSON.stringify({
      ...draftProvider,
      id: providerId ?? undefined,
    }),
  ) as ProviderInput;
}
