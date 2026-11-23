import assert from "node:assert/strict";
import test from "node:test";
import { nextTick, ref } from "vue";
import type { Provider } from "../src/stores/providers.ts";
import { useProviderDetail } from "../src/composables/useProviderDetail.ts";

function provider(id: string) {
  return { identity: { id } } as unknown as Provider;
}

test("the detail surface closes itself when its provider disappears", async () => {
  const providers = ref<Provider[]>([provider("a"), provider("b")]);
  const detail = useProviderDetail(providers);

  detail.openProviderDetail(providers.value[0]);
  assert.equal(detail.providerDetailVisible.value, true);
  assert.equal(detail.providerDetailProvider.value?.identity.id, "a");

  // 后台刷新换了对象但 id 还在：详情继续跟着 store，不打断阅读。
  providers.value = [provider("a")];
  await nextTick();
  assert.equal(detail.providerDetailVisible.value, true);
  assert.equal(detail.providerDetailProvider.value?.identity.id, "a");

  // 删除或导入替换后目标消失，窗口不能停在一个没有主体的空壳上。
  providers.value = [provider("b")];
  await nextTick();
  assert.equal(detail.providerDetailVisible.value, false);
  assert.equal(detail.providerDetailProvider.value, null);
});

test("closing the detail clears the tracked id", () => {
  const providers = ref<Provider[]>([provider("a")]);
  const detail = useProviderDetail(providers);

  detail.openProviderDetail(providers.value[0]);
  detail.closeProviderDetail();
  assert.equal(detail.providerDetailVisible.value, false);
  assert.equal(detail.providerDetailProvider.value, null);

  // 关闭后重新打开同一项仍然可用。
  detail.openProviderDetail(providers.value[0]);
  assert.equal(detail.providerDetailVisible.value, true);
  assert.equal(detail.providerDetailProvider.value?.identity.id, "a");
});
