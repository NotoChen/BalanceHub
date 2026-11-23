import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { effectScope, ref, shallowRef } from "vue";
import type {
  BatchOperationResult,
  ProviderBatchProgressEvent,
  ProviderBatchProgressItem,
  ProviderBatchStatus,
} from "../src/api/batch-operation.ts";
import type { Provider } from "../src/stores/provider-types.ts";
import { mergeProvidersByRevision } from "../src/utils/provider-revision.ts";
import { loadSource } from "./helpers/load-source.ts";

type BatchModule = typeof import("../src/composables/useBatchOperation.ts");

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

function provider(id: string, revision = 1, available = 10): Provider {
  return {
    identity: { id, name: id, baseUrl: `https://${id}.invalid` },
    revision,
    runtime: { enabled: true, status: "ok", errorMessage: null },
    quota: { available },
  } as Provider;
}

function item(
  provider: Provider,
  status: ProviderBatchStatus,
): ProviderBatchProgressItem {
  return {
    providerId: provider.identity.id,
    name: provider.identity.name,
    baseUrl: provider.identity.baseUrl,
    status,
    message: status,
  };
}

function harness(
  initial: Provider[],
  refreshCliRuntime = async (): Promise<unknown> => undefined,
) {
  const providers = shallowRef(initial);
  const busy = ref(false);
  const messages: string[] = [];
  const infos: string[] = [];
  const cancellations: string[] = [];
  const tombstones: Record<string, number> = {};
  const requests: Array<
    ReturnType<typeof deferred<BatchOperationResult>> & {
      send: (event: ProviderBatchProgressEvent) => void;
    }
  > = [];
  const { useBatchOperation } = loadSource<BatchModule>(
    "composables/useBatchOperation.ts",
    {
      "@arco-design/web-vue": {
        Message: {
          error: (message: string) => messages.push(message),
          info: (message: string) => infos.push(message),
        },
      },
      "../api/transport.ts": {
        Channel: class {
          onmessage: (event: ProviderBatchProgressEvent) => void;
          constructor(onmessage: (event: ProviderBatchProgressEvent) => void) {
            this.onmessage = onmessage;
          }
        },
      },
      "../api/batch-operation": {
        cancelProviderRefresh: async () => {
          cancellations.push("cancel");
        },
        refreshAllProvidersWithProgress: (channel: {
          onmessage: (event: ProviderBatchProgressEvent) => void;
        }) => {
          const request = {
            ...deferred<BatchOperationResult>(),
            send: channel.onmessage,
          };
          requests.push(request);
          return request.promise;
        },
      },
    },
  );
  const scope = effectScope();
  const controller = scope.run(() =>
    useBatchOperation({
      providers,
      replaceProviders: (next) => {
        providers.value = next;
      },
      upsertProviders: (next) => {
        providers.value = mergeProvidersByRevision(
          providers.value,
          next,
          1,
          tombstones,
        );
      },
      setRefreshInProgress: (value) => {
        busy.value = value;
      },
      refreshCliRuntime,
    }),
  )!;
  const send = (event: ProviderBatchProgressEvent) =>
    requests.at(-1)!.send(event);
  return {
    ...controller,
    providers,
    busy,
    messages,
    infos,
    cancellations,
    requests,
    scope,
    tombstones,
    card: (id: string) =>
      providers.value.find((provider) => provider.identity.id === id)!,
    queued: (reason: string) =>
      send({
        event: "queued",
        data: { operation: "refresh", reason },
      }),
    started: () =>
      send({
        event: "started",
        data: {
          operation: "refresh",
          total: initial.length,
          items: initial.map((provider) =>
            item(provider, provider.runtime.enabled ? "pending" : "skipped"),
          ),
        },
      }),
    start: (provider: Provider) =>
      send({
        event: "providerStarted",
        data: { operation: "refresh", item: item(provider, "running") },
      }),
    finish: (provider: Provider | null, row: ProviderBatchProgressItem) =>
      send({
        event: "providerFinished",
        data: { operation: "refresh", item: row, provider },
      }),
    resolve: (
      updatedProviders: Provider[],
      items = updatedProviders.map((provider) => item(provider, "success")),
    ) => {
      requests.at(-1)!.resolve({ updatedProviders, items });
    },
  };
}

test("completed cards update while the final provider is still running and the progress modal is closed", async () => {
  const original = [provider("first"), provider("second"), provider("slow")];
  const controller = harness(original);
  const pending = controller.runRefresh();
  controller.started();
  original.forEach(controller.start);
  controller.visible.value = false;
  const updated = [provider("first", 2, 42), provider("second", 3, 84)];
  updated.forEach((provider) =>
    controller.finish(provider, item(provider, "success")),
  );

  assert.equal(controller.running.value, true);
  assert.equal(controller.busy.value, true);
  assert.equal(controller.visible.value, false);
  assert.deepEqual(
    controller.providers.value.map((provider) => provider.runtime.status),
    ["ok", "ok", "syncing"],
  );
  assert.deepEqual(
    controller.providers.value.map((provider) => provider.quota.available),
    [42, 84, 10],
  );
  assert.deepEqual(
    controller.items.value.map((item) => item.status),
    ["success", "success", "running"],
  );
  await controller.runRefresh();
  assert.equal(controller.requests.length, 1);
  assert.equal(controller.visible.value, true);
  const slow = provider("slow", 4, 21);
  controller.finish(slow, item(slow, "success"));
  controller.resolve([...updated, slow]);
  await pending;
  assert.equal(controller.busy.value, false);
  assert.equal(controller.running.value, false);
  controller.scope.stop();
});

test("queued and disabled cards keep their previous state until the backend starts that provider", async () => {
  const initial = Array.from({ length: 8 }, (_, index) =>
    provider(`provider-${index}`),
  );
  initial[7].runtime.enabled = false;
  initial[6].runtime.status = "warning";
  initial[6].runtime.errorMessage = "原有提醒";
  const controller = harness(initial);
  const pending = controller.runRefresh();
  controller.started();
  assert.ok(
    controller.providers.value.every(
      (provider) => provider.runtime.status !== "syncing",
    ),
  );
  initial.slice(0, 6).forEach(controller.start);
  assert.equal(
    controller.providers.value.filter(
      (provider) => provider.runtime.status === "syncing",
    ).length,
    6,
  );
  assert.equal(controller.card("provider-6").runtime.status, "warning");
  assert.equal(controller.card("provider-6").runtime.errorMessage, "原有提醒");
  assert.equal(controller.card("provider-7").runtime.status, "ok");
  controller.scope.stop();
  await pending;
  assert.ok(
    controller.providers.value.every(
      (provider) => provider.runtime.status !== "syncing",
    ),
  );
});

test("batch failure preserves finished, queued, newly added, and disabled cards", async () => {
  const [first, slow, queued, disabled] = [
    "first",
    "slow",
    "queued",
    "disabled",
  ].map((id) => provider(id));
  disabled.runtime.enabled = false;
  const controller = harness([first, slow, queued, disabled]);
  const pending = controller.runRefresh();
  controller.started();
  controller.start(first);
  controller.start(slow);
  const finished = provider("first", 2, 55);
  controller.finish(finished, item(finished, "success"));
  controller.providers.value = [
    ...controller.providers.value,
    provider("added", 3),
  ];
  controller.requests[0].reject(new Error("连接中断"));
  await pending;

  assert.equal(controller.card("first").quota.available, 55);
  assert.equal(controller.card("first").runtime.status, "ok");
  assert.equal(controller.card("slow").runtime.status, "error");
  assert.equal(controller.card("slow").runtime.errorMessage, "连接中断");
  for (const id of ["queued", "disabled", "added"])
    assert.equal(controller.card(id).runtime.status, "ok");
  assert.deepEqual(
    controller.items.value.map((item) => item.status),
    ["success", "failed", "failed", "skipped"],
  );
  assert.equal(controller.running.value, false);
  assert.equal(controller.busy.value, false);
  assert.equal(controller.messages.length, 1);
  controller.scope.stop();
});

test("late channel events and results after a timeout cannot affect the next refresh", async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const original = provider("site");
  const controller = harness([original]);
  const firstRun = controller.runRefresh();
  controller.started();
  controller.start(original);
  const firstRequest = controller.requests[0];
  context.mock.timers.tick(180_000);
  await firstRun;
  assert.equal(controller.busy.value, false);
  assert.equal(controller.card("site").runtime.status, "error");
  assert.match(controller.error.value, /进度长时间未更新/);

  const secondRun = controller.runRefresh();
  controller.started();
  firstRequest.send({
    event: "providerStarted",
    data: { operation: "refresh", item: item(original, "running") },
  });
  const stale = provider("site", 5, 999);
  firstRequest.send({
    event: "providerFinished",
    data: {
      operation: "refresh",
      item: item(stale, "success"),
      provider: stale,
    },
  });
  firstRequest.resolve({
    updatedProviders: [stale],
    items: [item(stale, "success")],
  });
  await Promise.resolve();
  assert.equal(controller.items.value[0].status, "pending");
  assert.equal(controller.card("site").quota.available, 10);
  const fresh = provider("site", 6, 50);
  controller.resolve([fresh]);
  await secondRun;
  assert.equal(controller.card("site").quota.available, 50);
  assert.equal(controller.items.value[0].status, "success");
  controller.scope.stop();
});

test("queued heartbeats keep the batch alive while waiting for the automatic refresh to finish", async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const original = provider("site");
  const controller = harness([original]);
  const pending = controller.runRefresh();
  context.mock.timers.tick(179_000);
  controller.queued("正在等待当前自动刷新结束，完成后立即开始");
  assert.match(controller.waitingReason.value, /自动刷新/);
  context.mock.timers.tick(179_000);
  await Promise.resolve();
  assert.equal(controller.running.value, true);
  assert.equal(controller.error.value, "");
  controller.started();
  assert.equal(controller.waitingReason.value, "", "the queued notice ends once providers start");
  controller.resolve([provider("site", 2)]);
  await pending;
  assert.equal(controller.running.value, false);
  assert.equal(controller.waitingReason.value, "");
  controller.scope.stop();
});

test("ongoing progress renews the timeout instead of imposing a fixed whole-batch deadline", async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const original = provider("site");
  const controller = harness([original]);
  const pending = controller.runRefresh();
  controller.started();
  context.mock.timers.tick(179_000);
  controller.start(original);
  context.mock.timers.tick(179_000);
  await Promise.resolve();
  assert.equal(controller.running.value, true);
  assert.equal(controller.error.value, "");
  controller.resolve([provider("site", 2)]);
  await pending;
  controller.scope.stop();
});

test("provider revisions and removal tombstones protect edits and deletions during incremental refresh", async () => {
  const original = [provider("edited", 5), provider("removed", 5)];
  const controller = harness(original);
  const pending = controller.runRefresh();
  controller.started();
  original.forEach(controller.start);
  const edited = provider("edited", 9, 72);
  edited.identity.name = "用户改名";
  controller.providers.value = [edited];
  controller.tombstones.removed = 9;
  const stale = [provider("edited", 8, 999), provider("removed", 8, 999)];
  stale.forEach((provider) =>
    controller.finish(provider, item(provider, "success")),
  );
  controller.resolve(stale);
  await pending;
  assert.deepEqual(controller.providers.value, [edited]);
  controller.scope.stop();
});

test("a save failure releases just that card and a skipped result restores the authoritative status", async () => {
  const initial = [provider("failed"), provider("changed")];
  const controller = harness(initial);
  const pending = controller.runRefresh();
  controller.started();
  initial.forEach(controller.start);
  controller.finish(null, {
    ...item(initial[0], "failed"),
    message: "保存失败",
  });
  assert.equal(controller.card("failed").runtime.status, "error");
  const changed = provider("changed", 3, 12);
  controller.finish(changed, item(changed, "skipped"));
  assert.equal(controller.card("changed").runtime.status, "ok");
  assert.equal(controller.card("changed").quota.available, 12);
  controller.requests[0].reject(new Error("保存失败"));
  await pending;
  assert.equal(controller.card("changed").runtime.status, "ok");
  controller.scope.stop();
});

test("the final response reconciles channel delivery order and CLI discovery never delays completion", async () => {
  const cli = deferred<void>();
  let cliCalls = 0;
  const original = provider("site");
  const controller = harness([original], () => {
    cliCalls++;
    return cli.promise;
  });
  const pending = controller.runRefresh();
  controller.started();
  controller.start(original);
  const fresh = provider("site", 2, 40);
  controller.resolve([fresh]);
  await pending;
  assert.equal(controller.running.value, false);
  assert.equal(controller.busy.value, false);
  assert.equal(controller.completed.value, true);
  assert.equal(controller.items.value[0].status, "success");
  assert.equal(controller.card("site").quota.available, 40);
  assert.equal(cliCalls, 1);
  controller.start(original);
  assert.equal(controller.card("site").runtime.status, "ok");
  cli.reject(new Error("独立扫描失败"));
  await Promise.resolve();
  assert.equal(controller.messages.length, 0);
  controller.scope.stop();
});

test("cancel stops queued providers but keeps the running batch state until the backend settles", async () => {
  const initial = [provider("first"), provider("second"), provider("third")];
  const controller = harness(initial);
  const pending = controller.runRefresh();
  controller.started();
  controller.start(initial[0]);
  await controller.cancelRefresh();
  const duringSettle = {
    cancellations: [...controller.cancellations],
    cancelled: controller.cancelled.value,
    canceling: controller.canceling.value,
    running: controller.running.value,
    busy: controller.busy.value,
  };

  initial.slice(1).forEach((entry) =>
    controller.finish(entry, {
      ...item(entry, "skipped"),
      message: "刷新已取消",
    }),
  );
  const first = provider("first", 2, 30);
  controller.finish(first, item(first, "success"));
  controller.resolve(
    [first, initial[1], initial[2]],
    [
      item(first, "success"),
      { ...item(initial[1], "skipped"), message: "刷新已取消" },
      { ...item(initial[2], "skipped"), message: "刷新已取消" },
    ],
  );
  await pending;
  assert.deepEqual(duringSettle, {
    cancellations: ["cancel"],
    cancelled: true,
    canceling: true,
    running: true,
    busy: true,
  });
  assert.equal(controller.canceling.value, false);
  assert.equal(controller.running.value, false);
  assert.equal(controller.busy.value, false);
  assert.equal(controller.completed.value, true);
  assert.deepEqual(
    controller.items.value.map((entry) => entry.status),
    ["success", "skipped", "skipped"],
  );
  controller.scope.stop();
});

test("批量刷新只给仍在队列里的卡片显示排队中", () => {
  const board = readFileSync(
    new URL("../src/components/ProviderBoard.vue", import.meta.url),
    "utf8",
  );
  const controller = readFileSync(
    new URL("../src/composables/useAppController.ts", import.meta.url),
    "utf8",
  );

  // 已经结束的卡片必须显示自己的成功/失败，不能被整个批次的忙碌状态盖住。
  assert.match(
    board,
    /refreshInProgress && refreshQueuedProviderIds\.has\(provider\.identity\.id\)/,
  );
  assert.doesNotMatch(board, /provider\.runtime\.status !== 'syncing'/);
  assert.match(controller, /item\.status === "pending"/);
  assert.match(controller, /refreshQueuedProviderIds/);
});

test("an empty or fully disabled board explains why no refresh started", async () => {
  const empty = harness([]);
  await empty.runRefresh();
  assert.deepEqual(empty.requests, []);
  assert.equal(empty.running.value, false);
  assert.equal(empty.visible.value, false);
  assert.deepEqual(empty.infos, ["没有已启用的中转站可刷新"]);

  const disabled = provider("off");
  disabled.runtime.enabled = false;
  const disabledBoard = harness([disabled]);
  await disabledBoard.runRefresh();
  assert.deepEqual(disabledBoard.requests, []);
  assert.equal(disabledBoard.visible.value, false);
  assert.deepEqual(disabledBoard.infos, ["没有已启用的中转站可刷新"]);
});

test("disposing the controller releases its state and ignores all late IPC work", async () => {
  const original = provider("site");
  original.runtime.status = "warning";
  original.runtime.errorMessage = "保留之前的状态";
  const controller = harness([original]);
  const pending = controller.runRefresh();
  controller.started();
  controller.start(original);
  controller.scope.stop();
  await pending;
  assert.equal(controller.running.value, false);
  assert.equal(controller.busy.value, false);
  assert.equal(controller.visible.value, false);
  assert.equal(controller.card("site").runtime.status, "warning");
  assert.equal(controller.card("site").runtime.errorMessage, "保留之前的状态");
  const late = provider("site", 9, 999);
  controller.finish(late, item(late, "success"));
  controller.resolve([late]);
  await controller.runRefresh();
  assert.equal(controller.requests.length, 1);
  assert.equal(controller.card("site").quota.available, 10);
  assert.equal(controller.messages.length, 0);
});
