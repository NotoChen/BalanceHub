import assert from "node:assert/strict";
import test, { before, type TestContext } from "node:test";
import { createRenderer, nextTick } from "vue";
import type { CheckInTask } from "../src/api/checkin.ts";
import type { CheckInSnapshot } from "../src/utils/check-in-tasks.ts";
import { useCheckInBatchProgress } from "../src/composables/useCheckInBatchProgress.ts";
import { loadSource } from "./helpers/load-source.ts";

// 组合式函数在挂载和卸载时操作 window 焦点监听；Node 测试进程没有 window。
before(() => {
  (globalThis as { window?: unknown }).window = {
    addEventListener() {},
    removeEventListener() {},
  };
});

function completedTask(runId: string, providerId: string): CheckInTask {
  return {
    runId, providerId, providerName: "站点 A", batchId: null, source: "manual",
    phase: "completed", message: "签到成功", revision: 1, finished: true,
    canResume: false, canCancel: false, canShowWindow: false,
    startedAt: 1, finishedAt: 2,
  };
}

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

test("history check-ins from the first backend list do not replay completion toasts", async (t) => {
  const messages: string[] = [];
  const reloads: string[][] = [];
  const reconnects: (() => void)[] = [];
  let publish!: (snapshot: CheckInSnapshot) => void;
  let refreshes = 0;

  const { useCheckInActions } = loadSource<{
    useCheckInActions: (options: {
      reload: (ids: string[]) => Promise<unknown>;
      browserRuntime: { state: { value: unknown }; open: () => void };
    }) => unknown;
  }>("composables/useCheckInActions.ts", {
    "@arco-design/web-vue": {
      Message: {
        info() {},
        success: (message: string) => messages.push(message),
        warning: (message: string) => messages.push(message),
        error() {},
      },
    },
    "../api/checkin": {
      checkInProvider: async () => completedTask("new", "provider-a"),
      checkInAllProviders: async () => ({ batchId: "batch", tasks: [], skipped: 0 }),
      listCheckInTasks: async () => [],
      listenCheckInTasks: async () => () => {},
      resumeCheckInTask: async () => completedTask("new", "provider-a"),
      cancelCheckInTask: async () => {},
      showCheckInWindow: async () => {},
    },
    "../api/transport.ts": {
      listen: async (event: string, callback: () => void) => {
        if (event === "balancehub-events-reconnected") reconnects.push(callback);
        return () => {};
      },
    },
    "../utils/check-in-tasks": {
      createCheckInTracker: (
        _api: unknown,
        changed: (snapshot: CheckInSnapshot) => void,
      ) => {
        publish = changed;
        return {
          start: async () => {},
          stop: () => {},
          refresh: async () => { refreshes += 1; },
          submit: async () => completedTask("new", "provider-a"),
          submitAll: async () => ({ batchId: "batch", tasks: [], skipped: 0 }),
          resume: async () => completedTask("new", "provider-a"),
          cancel: async () => {},
          showWindow: async () => {},
        };
      },
    },
    "./useCheckInBatchProgress": { useCheckInBatchProgress },
  });

  mount(t, () => {
    useCheckInActions({
      reload: async (ids) => { reloads.push(ids); },
      browserRuntime: { state: { value: null }, open: () => {} },
    });
  });
  await settle();
  assert.equal(reconnects.length, 1, "重连补偿事件只订阅一次");

  // 前端重载后的第一份后端任务全集：已结束的历史任务不能重新弹提示。
  publish({ items: [completedTask("old", "provider-a")], pending: [], error: "", listed: true });
  await nextTick();
  assert.deepEqual(messages, []);
  assert.deepEqual(reloads, []);

  // 之后真正新完成的任务仍要提示并触发站点数据重读。
  publish({
    items: [completedTask("old", "provider-a"), completedTask("fresh", "provider-b")],
    pending: [], error: "", listed: true,
  });
  await nextTick();
  assert.deepEqual(messages, ["签到成功"]);
  assert.deepEqual(reloads, [["provider-b"]]);

  // 事件流重连后自动补读后端任务全集，避免错过终态事件后一直显示运行中。
  reconnects[0]();
  await settle();
  assert.equal(refreshes, 1);
});
