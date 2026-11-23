import assert from "node:assert/strict";
import test from "node:test";
import type { ProviderProtocolDetectionResult } from "../src/stores/provider-types.ts";
import { deferred, providerEditorFixture, settle } from "./helpers/provider-editor-fixture.ts";

test("clearing the address while a site probe is running releases the busy state", async (t) => {
  const pending = deferred<ProviderProtocolDetectionResult>();
  const context = providerEditorFixture(t, { detect: () => pending.promise });
  await settle();

  const first = context.editor.probeSite({ force: true });
  await settle();
  assert.equal(context.editor.probingSite.value, true);

  // 用户清空地址后失焦：新的探测无法开始，但它已经让上一次探测过期。
  // 上一次探测回来时不会再清除读取态，所以新的探测必须自己收口，否则
  // 面板会一直显示“识别中”，协议选择和重新识别都被锁住。
  context.editor.draftProvider.identity.baseUrl = "";
  await context.editor.probeSite({ silent: true });
  assert.equal(
    context.editor.probingSite.value,
    false,
    "空地址的探测不能把上一个探测的读取态永久挂住",
  );

  pending.resolve({
    detectedProtocol: null,
    message: "fixture",
    site: null,
    ambiguous: false,
    candidates: [],
  });
  await first;
  await settle();
  assert.equal(context.editor.probingSite.value, false);
  assert.equal(
    context.editor.protocolDetectionResult.value,
    null,
    "过期探测的识别结果不能回写",
  );
});
