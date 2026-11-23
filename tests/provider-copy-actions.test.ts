import assert from "node:assert/strict";
import test from "node:test";
import type { Provider } from "../src/stores/provider-types.ts";
import { loadSource } from "./helpers/load-source.ts";

type CopyActions = {
  copyInviteLink: (provider: Provider) => Promise<void>;
};

function setup(behavior: {
  getInviteLink?: () => Promise<string>;
  reload?: () => Promise<unknown>;
  copy?: (value: string) => Promise<void>;
}) {
  const messages: { level: string; text: string }[] = [];
  const copied: string[] = [];
  const busy: (string | null)[] = [];
  const record = (level: string) => (message: string) => {
    messages.push({ level, text: message });
  };
  const { useProviderCopyActions } = loadSource<{
    useProviderCopyActions: (options: Record<string, unknown>) => CopyActions;
  }>("composables/useProviderCopyActions.ts", {
    "@arco-design/web-vue": {
      Message: {
        success: record("success"),
        warning: record("warning"),
        error: record("error"),
      },
    },
    "./useClipboard": {
      copyText: async (value: string) => {
        copied.push(value);
        await behavior.copy?.(value);
      },
    },
  });
  const actions = useProviderCopyActions({
    getInviteLink: behavior.getInviteLink ?? (async () => "https://site.test/invite/abc"),
    reload: behavior.reload ?? (async () => undefined),
    setBusyProviderId: (id: string | null) => busy.push(id),
  });
  return { actions, messages, copied, busy };
}

function gatewayProvider(inviteLink = ""): Provider {
  return {
    identity: {
      id: "provider-a",
      name: "站点 A",
      protocol: "newapi",
      baseUrl: "https://site.test",
    },
    auth: { apiKey: "", accessToken: "", sessionCookie: "" },
    capabilities: { inviteLink },
    runtime: { enabled: true },
  } as unknown as Provider;
}

test("a copied invite link is reported even when the following reload fails", async () => {
  const { actions, messages, copied, busy } = setup({
    reload: async () => {
      throw new Error("读取配置失败");
    },
  });

  await actions.copyInviteLink(gatewayProvider());

  assert.deepEqual(copied, ["https://site.test/invite/abc"]);
  assert.equal(
    messages.filter((message) => message.level === "success").length,
    1,
    "the copy itself is not hidden by the failed reload",
  );
  assert.match(
    messages.find((message) => message.level === "warning")?.text ?? "",
    /邀请链接已复制，但列表刷新失败：读取配置失败/,
  );
  assert.equal(
    messages.some((message) => message.level === "error"),
    false,
    "a successful copy is not reported as a failure",
  );
  assert.deepEqual(busy, ["provider-a", null]);
});

test("an invite link that cannot be read fails without claiming a copy", async () => {
  const { actions, messages, copied, busy } = setup({
    getInviteLink: async () => {
      throw new Error("站点拒绝生成邀请链接");
    },
  });

  await actions.copyInviteLink(gatewayProvider());

  assert.deepEqual(copied, []);
  assert.equal(messages.some((message) => message.level === "success"), false);
  assert.match(
    messages.find((message) => message.level === "error")?.text ?? "",
    /站点拒绝生成邀请链接/,
  );
  assert.deepEqual(busy, ["provider-a", null]);
});

test("a clipboard failure keeps the cached link and releases the card", async () => {
  const { actions, messages, busy } = setup({
    copy: async () => {
      throw new Error("剪贴板不可用");
    },
  });

  await actions.copyInviteLink(gatewayProvider("https://site.test/invite/cached"));

  assert.match(
    messages.find((message) => message.level === "error")?.text ?? "",
    /剪贴板不可用/,
  );
  assert.equal(messages.some((message) => message.level === "success"), false);
  assert.deepEqual(busy, ["provider-a", null]);
});
