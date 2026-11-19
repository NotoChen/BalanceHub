import assert from "node:assert/strict";
import test from "node:test";
import { effectScope, nextTick, ref } from "vue";
import type { ModelMetadataSnapshot } from "../src/api/model-catalog.ts";
import { loadSource } from "./helpers/load-source.ts";

interface Deferred<T> {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (error: Error) => void;
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

async function settle() {
  for (let index = 0; index < 16; index += 1) await Promise.resolve();
  await nextTick();
}

/// 只关心快照属于哪个站点；其余字段不参与这条链路。
function snapshot(id: string): ModelMetadataSnapshot {
  return {
    models: [{ id } as ModelMetadataSnapshot["models"][number]],
    updatedAt: "1000",
    stale: false,
    error: null,
  };
}

function loadComposable(
  getModelMetadata: (
    ids: string[],
    refresh: boolean,
    providerId?: string,
  ) => Promise<ModelMetadataSnapshot>,
) {
  return loadSource<typeof import("../src/composables/useModelMetadata.ts")>(
    "composables/useModelMetadata.ts",
    {
      "../api/transport.ts": { listen: async () => () => {} },
      "../api/model-catalog": { getModelMetadata },
      "../utils/promise-timeout": {
        withTimeout: (promise: Promise<unknown>) => promise,
      },
    },
  ).useModelMetadata;
}

test("切换站点时先清空旧快照，不能沿用上一张卡的分组价", async (t) => {
  const second = deferred<ModelMetadataSnapshot>();
  const calls: Array<string | undefined> = [];
  const useModelMetadata = loadComposable((_ids, _refresh, providerId) => {
    calls.push(providerId);
    return providerId === "a"
      ? Promise.resolve(snapshot("a-price"))
      : second.promise;
  });

  const scope = effectScope();
  t.after(() => scope.stop());
  const ids = ref(["gpt-5"]);
  const active = ref(true);
  const providerId = ref<string | undefined>("a");
  const metadata = scope.run(() => useModelMetadata(ids, active, providerId))!;

  await settle();
  assert.equal(metadata.snapshot.value?.models[0]?.id, "a-price");

  providerId.value = "b";
  await settle();
  // 新站点的请求还没回来，旧站点按分组换算的价目不能先显示出来。
  assert.equal(metadata.snapshot.value, null);
  assert.equal(metadata.loading.value, true);
  assert.deepEqual(calls, ["a", "b"]);

  second.resolve(snapshot("b-price"));
  await settle();
  assert.equal(metadata.snapshot.value?.models[0]?.id, "b-price");
  assert.equal(metadata.loading.value, false);
});

test("切站后晚到的旧资料不会回写", async (t) => {
  const first = deferred<ModelMetadataSnapshot>();
  const second = deferred<ModelMetadataSnapshot>();
  const useModelMetadata = loadComposable((_ids, _refresh, providerId) =>
    providerId === "a" ? first.promise : second.promise,
  );

  const scope = effectScope();
  t.after(() => scope.stop());
  const ids = ref(["gpt-5"]);
  const active = ref(true);
  const providerId = ref<string | undefined>("a");
  const metadata = scope.run(() => useModelMetadata(ids, active, providerId))!;

  await settle();
  providerId.value = "b";
  await settle();

  first.resolve(snapshot("a-price"));
  await settle();
  assert.equal(metadata.snapshot.value, null, "切站后旧请求不能回写");
  assert.equal(metadata.loading.value, true);

  second.resolve(snapshot("b-price"));
  await settle();
  assert.equal(metadata.snapshot.value?.models[0]?.id, "b-price");
});

test("换调用 Key 后旧价目立即失效并按新凭据重新读取", async (t) => {
  const first = deferred<ModelMetadataSnapshot>();
  const second = deferred<ModelMetadataSnapshot>();
  let calls = 0;
  const useModelMetadata = loadComposable(() => {
    calls += 1;
    return calls === 1 ? first.promise : second.promise;
  });

  const scope = effectScope();
  t.after(() => scope.stop());
  const ids = ref(["gpt-5"]);
  const active = ref(true);
  const providerId = ref<string | undefined>("a");
  const credentialRevision = ref(1);
  const metadata = scope.run(() =>
    useModelMetadata(ids, active, providerId, credentialRevision),
  )!;

  await settle();
  first.resolve(snapshot("old-key"));
  await settle();
  assert.equal(metadata.snapshot.value?.models[0]?.id, "old-key");

  credentialRevision.value = 2;
  await settle();
  // 站点价目按 Key 分组换算，换 Key 后旧快照连展示都不应该继续。
  assert.equal(metadata.snapshot.value, null);
  assert.equal(calls, 2, "换 Key 要重新读取当前凭据的站点价目");

  second.resolve(snapshot("new-key"));
  await settle();
  assert.equal(metadata.snapshot.value?.models[0]?.id, "new-key");
  assert.equal(metadata.loading.value, false);
});

test("关闭后清空资料并丢弃在途结果，重新打开会重新读取", async (t) => {
  const first = deferred<ModelMetadataSnapshot>();
  const second = deferred<ModelMetadataSnapshot>();
  let calls = 0;
  const useModelMetadata = loadComposable(() => {
    calls += 1;
    return calls === 1 ? first.promise : second.promise;
  });

  const scope = effectScope();
  t.after(() => scope.stop());
  const ids = ref(["gpt-5"]);
  const active = ref(true);
  const providerId = ref<string | undefined>("a");
  const metadata = scope.run(() => useModelMetadata(ids, active, providerId))!;

  await settle();
  first.resolve(snapshot("before-close"));
  await settle();
  assert.equal(metadata.snapshot.value?.models[0]?.id, "before-close");

  active.value = false;
  await settle();
  assert.equal(metadata.loading.value, false, "关闭后不能保留忙碌状态");
  assert.equal(metadata.snapshot.value, null, "关闭后旧站点的价目不能留到下次打开");

  active.value = true;
  await settle();
  assert.equal(calls, 2, "重新打开要按当前站点和凭据重新读取");
  second.resolve(snapshot("fresh"));
  await settle();
  assert.equal(metadata.snapshot.value?.models[0]?.id, "fresh");
  assert.equal(metadata.loading.value, false);
});
