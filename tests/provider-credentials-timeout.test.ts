import assert from "node:assert/strict";
import test, { type TestContext } from "node:test";
import { createRenderer, nextTick } from "vue";
import { loadSource } from "./helpers/load-source.ts";
import { withTimeout } from "../src/utils/promise-timeout.ts";

type Controller = {
  visible: { value: boolean };
  details: { value: unknown };
  pending: { value: string };
  error: { value: string };
  open: (id: string) => void;
  refresh: () => Promise<void>;
};

function mount(t: TestContext, setup: () => void) {
  const renderer = createRenderer<object, object>({
    patchProp() {},
    insert() {},
    remove() {},
    createElement: () => ({}),
    createText: () => ({}),
    createComment: () => ({}),
    setText() {},
    setElementText() {},
    parentNode: () => null,
    nextSibling: () => null,
  });
  const app = renderer.createApp({
    setup() {
      setup();
      return () => null;
    },
  });
  app.mount({});
  let active = true;
  const dispose = () => {
    if (active) {
      active = false;
      app.unmount();
    }
  };
  t.after(dispose);
  return { dispose };
}

async function settle() {
  for (let index = 0; index < 20; index += 1) await Promise.resolve();
  await nextTick();
}

test("a credential read that never returns releases its busy state after the timeout", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { useProviderCredentials } = loadSource<{
    useProviderCredentials: (relogin: (id: string) => void) => Controller;
  }>("composables/useProviderCredentials.ts", {
    "@arco-design/web-vue": {
      Message: { success() {}, error() {}, warning() {} },
      Modal: { confirm() {} },
    },
    "../api/login-accounts": {
      getProviderCredentials: () => new Promise(() => {}),
      validateProviderCredentials: async () => null,
      clearProviderCredential: async () => {},
    },
    "../utils/promise-timeout": { withTimeout },
  });
  let controller!: Controller;
  mount(t, () => {
    controller = useProviderCredentials(() => {});
  });

  controller.open("provider-a");
  await settle();
  assert.equal(controller.pending.value, "load", "读取开始后按钮进入忙碌态");

  // IPC 一直不返回时，超时必须自己收口，否则刷新、验证和清除按钮会永远禁用。
  t.mock.timers.tick(30_000);
  await settle();
  assert.equal(controller.pending.value, "");
  assert.equal(controller.error.value, "读取站点凭据超时，请重试");
  assert.equal(controller.details.value, null);
});
