import assert from "node:assert/strict";
import test, { type TestContext } from "node:test";
import type { Provider } from "../src/stores/provider-types.ts";
import { loadSource } from "./helpers/load-source.ts";

type RenewController = {
  renewAllCpaAccounts: (provider: Provider) => void;
};

type ConfirmOptions = { onOk?: () => Promise<void> | void };

function cpaProvider(): Provider {
  return {
    identity: { id: "provider-a", name: "站点 A", protocol: "newapi" },
    auth: { credentialRevision: 3 },
    actions: { cpaGateway: true },
    runtime: { enabled: true },
  } as unknown as Provider;
}

function setup(
  t: TestContext,
  behavior: {
    renew: () => Promise<unknown>;
    reload?: () => Promise<unknown>;
  },
) {
  const messages: { level: string; text: string }[] = [];
  let confirm: ConfirmOptions | undefined;
  const record = (level: string) => (message: string) => {
    messages.push({ level, text: message });
  };
  const { useProviderActions } = loadSource<{
    useProviderActions: (options: Record<string, unknown>) => RenewController;
  }>("composables/useProviderActions.ts", {
    "@arco-design/web-vue": {
      Message: {
        info: record("info"),
        success: record("success"),
        warning: record("warning"),
        error: record("error"),
      },
      Modal: {
        confirm: (options: ConfirmOptions) => {
          confirm = options;
        },
      },
    },
    "../api/oauth-accounts": {
      refreshAllCpa: behavior.renew,
      startOAuthLogin: async () => {
        throw new Error("unused");
      },
    },
    "../stores/oauth-accounts": { useOAuthAccountStore: () => ({}) },
    "../utils/provider-api-key-options": { hasUsableProviderApiKey: () => true },
    "./useProviderCopyActions": {
      useProviderCopyActions: () => ({
        copyProviderUrl: async () => undefined,
        copyProviderSecret: async () => undefined,
        copyInviteLink: async () => undefined,
      }),
    },
    "./useProviderAppImport": {
      useProviderAppImport: () => ({ addProviderToApp: () => undefined }),
    },
    "../utils/provider-display": {
      providerDisplayLabel: (provider: Provider) => provider.identity.name,
    },
    "../utils/provider-protocol": {
      providerScopeNoun: () => "中转站",
    },
  });

  const controller = useProviderActions({
    providers: { value: [] },
    refreshByIds: async () => undefined,
    openWorkspacePicker: () => undefined,
    probeCapabilities: async () => ({ provider: cpaProvider(), message: "" }),
    getInviteLink: async () => "",
    reloadProvider: behavior.reload ?? (async () => undefined),
    openEditProvider: () => undefined,
    checkInProviderAction: async () => undefined,
    openApiKeyManager: () => undefined,
    openAvailableModels: () => undefined,
    openUsage: () => undefined,
    openRequestLogs: () => undefined,
    openPasswordChange: () => undefined,
    openAccountProfile: () => undefined,
    openCheckInRecords: () => undefined,
    reloginProvider: async () => undefined,
    toggleProvider: async () => undefined,
    removeProvider: async () => undefined,
  });
  controller.renewAllCpaAccounts(cpaProvider());
  t.after(() => {
    confirm = undefined;
  });
  return {
    messages,
    confirm: () => {
      assert.ok(confirm?.onOk, "续期前必须经过确认弹窗");
      return confirm.onOk;
    },
  };
}

test("a rejected CPA renewal request surfaces the failure instead of stalling the dialog", async (t) => {
  const { messages, confirm } = setup(t, {
    renew: async () => {
      throw new Error("CPA 拒绝了这次续期");
    },
  });

  // onOk 必须自己消化错误，拒绝的 Promise 会让确认弹窗停在原地。
  await confirm()();

  assert.deepEqual(
    messages.map((message) => message.level),
    ["error"],
  );
  assert.match(messages[0].text, /请求 CPA 全部续期失败/);
  assert.match(messages[0].text, /CPA 拒绝了这次续期/);
});

test("a failed panel reload after a successful renewal still reports what happened", async (t) => {
  const { messages, confirm } = setup(t, {
    renew: async () => ({}),
    reload: async () => {
      throw new Error("IPC 超时");
    },
  });

  await confirm()();

  assert.deepEqual(
    messages.map((message) => message.level),
    ["success", "warning"],
  );
  assert.match(messages[1].text, /已请求续期，但面板刷新失败/);
  assert.match(messages[1].text, /IPC 超时/);
});
