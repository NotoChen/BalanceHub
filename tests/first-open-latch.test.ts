import assert from "node:assert/strict";
import test from "node:test";
import { nextTick, ref } from "vue";
import { useFirstOpenLatch } from "../src/composables/useFirstOpenLatch.ts";

test("首次打开后保持挂载，关闭不回收", async () => {
  const visible = ref(false);
  const opened = useFirstOpenLatch(() => visible.value);

  assert.equal(opened.value, false);
  visible.value = true;
  // 打开动作同步置位可见标志，闸门必须在同一拍打开，异步组件才赶得上第一次打开。
  assert.equal(opened.value, true);
  await nextTick();

  visible.value = false;
  await nextTick();
  assert.equal(opened.value, true);
});

test("挂载时已经可见就直接打开", async () => {
  const visible = ref(true);
  const opened = useFirstOpenLatch(() => visible.value);

  assert.equal(opened.value, true);
  visible.value = false;
  await nextTick();
  assert.equal(opened.value, true);
});
