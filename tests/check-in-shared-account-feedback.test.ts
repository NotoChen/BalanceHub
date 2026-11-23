import assert from "node:assert/strict";
import test from "node:test";
import { effectScope, shallowRef } from "vue";
import type { CheckInTask } from "../src/api/checkin.ts";
import type { Provider } from "../src/stores/provider-types.ts";
import type { CheckInSnapshot } from "../src/utils/check-in-tasks.ts";
import { useCheckInBatchProgress } from "../src/composables/useCheckInBatchProgress.ts";
import { loadSource } from "./helpers/load-source.ts";

function runningTask(): CheckInTask {
  return {
    runId: "run-a", providerId: "provider-a", providerName: "站点 A", batchId: null,
    source: "manual", phase: "checking", message: "正在请求签到", revision: 2,
    finished: false, canResume: false, canCancel: true, canShowWindow: false,
    startedAt: 1, finishedAt: null,
  };
}

test("a shared login account explains why the second card produced no task", async (t) => {
  // 组合式函数在组件外执行时会打印生命周期警告；这里只验证动作反馈。
  const warn = console.warn;
  console.warn = () => undefined;
  t.after(() => { console.warn = warn; });
  const messages: string[] = [];
  const { useCheckInActions } = loadSource<{
    useCheckInActions: (options: {
      reload: (ids: string[]) => Promise<unknown>;
      browserRuntime: unknown;
    }) => { checkInProviderAction: (provider: Provider) => Promise<void> };
  }>("composables/useCheckInActions.ts", {
    "@arco-design/web-vue": {
      Message: {
        info: (message: string) => messages.push(message),
        success: () => undefined,
        warning: () => undefined,
        error: () => undefined,
      },
    },
    "../api/checkin": {
      // 后端按「登录账号」复用任务：第二张卡片的提交返回第一张卡片的运行中任务。
      checkInProvider: async () => runningTask(),
      checkInAllProviders: async () => ({ batchId: "batch", tasks: [], skipped: 0 }),
      listCheckInTasks: async () => [],
      listenCheckInTasks: async () => () => undefined,
      resumeCheckInTask: async () => runningTask(),
      cancelCheckInTask: async () => undefined,
      showCheckInWindow: async () => undefined,
    },
    "../api/transport.ts": {
      listen: async () => () => undefined,
    },
    "../utils/check-in-tasks": {
      // 只验证卡片动作对「后端复用了另一张卡片的任务」的反馈，不依赖真实跟踪器。
      createCheckInTracker: (
        _api: unknown,
        changed: (snapshot: CheckInSnapshot) => void,
      ) => {
        const task = runningTask();
        const publish = () => changed({ items: [task], pending: [], error: "", listed: true });
        return {
          start: async () => publish(),
          stop: () => undefined,
          refresh: async () => publish(),
          submit: async () => {
            publish();
            return task;
          },
          submitAll: async () => ({ batchId: "batch", tasks: [], skipped: 0 }),
          resume: async () => task,
          cancel: async () => undefined,
          showWindow: async () => undefined,
        };
      },
    },
    "./useCheckInBatchProgress": { useCheckInBatchProgress },
  });

  const scope = effectScope();
  const controller = scope.run(() =>
    useCheckInActions({
      reload: async () => undefined,
      browserRuntime: { state: shallowRef(null), open: () => undefined },
    }),
  )!;
  const provider = {
    identity: { id: "provider-b", name: "站点 B" },
    auth: { apiKey: "", apiKeyOptions: [] },
  } as unknown as Provider;

  await controller.checkInProviderAction(provider);
  scope.stop();

  assert.equal(messages.length, 1);
  assert.match(messages[0], /该登录账号正在「站点 A」签到/);
  assert.match(messages[0], /正在请求签到/);
});
