import assert from "node:assert/strict";
import test, { type TestContext } from "node:test";
import { ref } from "vue";
import { useSessionReader } from "../src/composables/useSessionReader.ts";
import type { AgentSessionQueryApi } from "../src/api/agent-sessions.ts";
import type {
  AgentSessionCancelRequest,
  AgentSessionDetail,
  AgentSessionDetailRequest,
} from "../src/stores/agent-session-types.ts";
import type {
  AgentSessionMessageContent,
  AgentSessionMessageRequest,
} from "../src/stores/session-reader-types.ts";
import {
  sessionDetail,
  sessionMessage,
  sessionRow,
  sessionScope,
} from "./agent-session-fixtures.ts";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
async function settle() {
  for (let pass = 0; pass < 12; pass += 1) await Promise.resolve();
}

function harness(
  t: TestContext,
  overrides: Partial<AgentSessionQueryApi> = {},
) {
  const requests: AgentSessionDetailRequest[] = [];
  const chunks: AgentSessionMessageRequest[] = [];
  const cancellations: AgentSessionCancelRequest[] = [];
  const active = ref(true);
  const scope = ref(sessionScope());
  const rows = ref([sessionRow()]);
  const query = ref("");
  const reader = useSessionReader({
    instance: "reader-regression",
    active,
    scope,
    rows,
    query,
    contextKey: ref("context"),
    parentRevision: () => 0,
    api: {
      scope: async () => sessionScope(),
      query: async () => {
        throw new Error("unexpected list read");
      },
      detail: async (request) => {
        requests.push(request);
        return overrides.detail ? overrides.detail(request) : sessionDetail();
      },
      message: async (request) => {
        chunks.push(request);
        if (overrides.message) return overrides.message(request);
        throw new Error("unexpected message read");
      },
      cancel: async (request) => {
        cancellations.push(request);
      },
    },
  });
  t.after(reader.closeDetail);
  return {
    reader,
    requests,
    chunks,
    cancellations,
    active,
    scope,
    rows,
    query,
  };
}
function detailWithLongMessage(
  kind: "text" | "image" = "text",
  contentVersion = 1,
) {
  const detail = sessionDetail();
  const message = sessionMessage("long-message", "开头", {
    contentLength: 8,
    hasMoreContent: true,
    contentVersion,
  });
  message.metadata.kind = kind;
  detail.detail.messages = [message];
  detail.detail.page.totalMessages = 1;
  return detail;
}
function chunk(
  request: AgentSessionMessageRequest,
  content: string,
  nextOffset: number | null,
  totalChars = 8,
): AgentSessionMessageContent {
  return {
    messageId: request.messageId,
    content,
    offset: request.offset,
    totalChars,
    nextOffset,
    changes: [],
  };
}

test("closing a reader releases busy state before its pending detail resolves", async (t) => {
  const pending = deferred<AgentSessionDetail>();
  const context = harness(t, { detail: async () => pending.promise });
  const opened = context.reader.openDetail("ref:session-1");
  assert.equal(context.reader.detailLoading.value, true);
  context.reader.closeDetail();
  assert.equal(context.reader.detailLoading.value, false);
  assert.equal(context.reader.detailVisible.value, false);
  assert.equal(
    context.cancellations[0].requestId,
    context.requests[0].requestId,
  );
  pending.resolve(sessionDetail());
  await opened;
  assert.equal(context.reader.detail.value, null);
});

test("search and page navigation keep the current page visible and reject older replies", async (t) => {
  const old = deferred<AgentSessionDetail>();
  const current = deferred<AgentSessionDetail>();
  let reads = 0;
  const context = harness(t, {
    detail: async () =>
      ++reads === 1
        ? sessionDetail()
        : reads === 2
          ? old.promise
          : current.promise,
  });
  await context.reader.openDetail("ref:session-1");
  const page = context.reader.navigateDetail({
    cursor: "revision:80",
    query: "关键词",
  });
  assert.equal(context.reader.detail.value?.messages.length, 3);
  const search = context.reader.navigateDetail({
    query: "新关键词",
    matchPosition: 2,
  });
  assert.equal(context.requests.at(-1)?.cursor, null);
  assert.equal(context.requests.at(-1)?.matchPosition, 2);
  const result = sessionDetail();
  result.detail.messages[0].content = "新查询正文";
  current.resolve(result);
  await search;
  old.resolve(sessionDetail());
  await page;
  assert.equal(context.reader.detail.value?.messages[0].content, "新查询正文");
  assert.equal(context.reader.detailLoading.value, false);
});

test("a list search opens its backend message anchor and retains the exact query", async (t) => {
  const context = harness(t);
  context.query.value = "中文 needle";
  context.rows.value[0].searchHit = {
    messageId: "later-page-hit",
    offset: 12000,
    snippet: "needle",
    terms: ["needle"],
    kind: "text",
  };
  await context.reader.openDetail("ref:session-1");
  assert.equal(context.requests[0].anchor, "later-page-hit");
  assert.equal(context.requests[0].query, "中文 needle");
});

test("refreshing a later page retains its first message when no search hit is selected", async (t) => {
  const result = sessionDetail();
  result.detail.page.start = 160;
  result.detail.messages = [sessionMessage("later-page-first", "后续页正文")];
  result.detail.find.activeMessageId = null;
  const context = harness(t, { detail: async () => result });
  await context.reader.openDetail("ref:session-1");
  await context.reader.refreshDetail();
  assert.equal(context.requests.at(-1)?.anchor, "later-page-first");
  assert.equal(context.requests.at(-1)?.cursor, null);
});

test("continuing a middle-of-message match appends its following chunk without rereading the start", async (t) => {
  const result = detailWithLongMessage();
  result.detail.messages[0] = sessionMessage("long-message", "命🦀中", {
    contentOffset: 2,
    contentLength: 8,
    hasMoreContent: true,
  });
  const context = harness(t, {
    detail: async () => result,
    message: async (request) => chunk(request, "后续文", null),
  });
  await context.reader.openDetail("ref:session-1");
  await context.reader.expandMessage("long-message");
  assert.deepEqual(
    context.chunks.map((request) => request.offset),
    [5],
  );
  assert.equal(
    context.reader.detail.value?.messages[0].content,
    "命🦀中后续文",
  );
  assert.equal(context.reader.detail.value?.messages[0].contentOffset, 2);
  assert.equal(context.reader.detail.value?.messages[0].hasMoreContent, false);
});

test("one image expansion reads every chunk with independent increasing request IDs", async (t) => {
  const context = harness(t, {
    detail: async () => detailWithLongMessage("image"),
    message: async (request) =>
      request.offset === 2
        ? chunk(request, "图🦀片", 5)
        : chunk(request, "的结尾", null),
  });
  await context.reader.openDetail("ref:session-1");
  await context.reader.expandMessage("long-message");
  assert.deepEqual(
    context.chunks.map((request) => request.offset),
    [2, 5],
  );
  assert.ok(context.chunks[1].requestId > context.chunks[0].requestId);
  assert.equal(context.chunks[0].contentVersion, 1);
  assert.equal(
    context.reader.detail.value?.messages[0].content,
    "开头图🦀片的结尾",
  );
  assert.equal(context.reader.detail.value?.messages[0].hasMoreContent, false);
  assert.equal(
    context.reader.messageStates.value["long-message"].loading,
    false,
  );
});

test("closing while automatic expansion is pending cancels it and never requests later chunks", async (t) => {
  const pending = deferred<AgentSessionMessageContent>();
  const context = harness(t, {
    detail: async () => detailWithLongMessage("image"),
    message: async (request) =>
      request.offset === 2 ? chunk(request, "图🦀片", 5) : pending.promise,
  });
  await context.reader.openDetail("ref:session-1");
  const expansion = context.reader.expandMessage("long-message");
  await settle();
  assert.equal(context.chunks.length, 2);
  assert.equal(
    context.reader.detailLoading.value,
    false,
    "expanding one message does not lock the reader",
  );
  context.reader.closeDetail();
  assert.deepEqual(context.reader.messageStates.value, {});
  assert.ok(
    context.cancellations.some(
      (request) => request.requestId === context.chunks[1].requestId,
    ),
  );
  pending.resolve(chunk(context.chunks[1], "的结尾", null));
  await expansion;
  assert.equal(context.reader.detail.value, null);
  assert.equal(context.chunks.length, 2);
});

test("message timeout releases local busy state and late content cannot overwrite a retry", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const pending = deferred<AgentSessionMessageContent>();
  let reads = 0;
  const context = harness(t, {
    detail: async () => detailWithLongMessage(),
    message: async (request) =>
      ++reads === 1 ? pending.promise : chunk(request, "重试后的正文", null),
  });
  await context.reader.openDetail("ref:session-1");
  const expansion = context.reader.expandMessage("long-message");
  t.mock.timers.tick(25_001);
  await expansion;
  assert.equal(
    context.reader.messageStates.value["long-message"].loading,
    false,
  );
  assert.match(
    context.reader.messageStates.value["long-message"].error,
    /超时/,
  );
  assert.equal(context.reader.detailVisible.value, true);
  await context.reader.expandMessage("long-message");
  pending.resolve(chunk(context.chunks[0], "过期后的正文", null));
  await settle();
  assert.equal(
    context.reader.detail.value?.messages[0].content,
    "开头重试后的正文",
  );
});

test("a mismatched chunk is rejected without appending duplicate or unrelated content", async (t) => {
  const context = harness(t, {
    detail: async () => detailWithLongMessage(),
    message: async (request) => ({
      ...chunk(request, "错误片段", 2),
      offset: 0,
    }),
  });
  await context.reader.openDetail("ref:session-1");
  await context.reader.expandMessage("long-message");
  assert.equal(context.reader.detail.value?.messages[0].content, "开头");
  assert.match(
    context.reader.messageStates.value["long-message"].error,
    /位置已变化/,
  );
  assert.equal(
    context.reader.messageStates.value["long-message"].loading,
    false,
  );
});

test("outline navigation preserves expanded diffs while native content stays unchanged", async (t) => {
  const changes = [
    {
      path: "src/example.ts",
      before: "old",
      after: "new",
      patch: null,
      beforeKnown: true,
    },
  ];
  for (const invalidation of ["none", "message", "source"]) {
    let reads = 0;
    const context = harness(t, {
      detail: async () => {
        const next = reads++ > 0;
        const detail = detailWithLongMessage(
          "text",
          next && invalidation === "message" ? 2 : 1,
        );
        if (next) detail.detail.messages[0].metadata.status = "failed";
        if (next && invalidation === "source")
          detail.detail.page.revision = "revision:changed";
        return detail;
      },
      message: async (request) => ({
        ...chunk(request, "已经展开正文", null),
        changes,
      }),
    });
    await context.reader.openDetail("ref:session-1");
    await context.reader.expandMessage("long-message");
    await context.reader.navigateDetail({ anchor: "long-message" });
    const message = context.reader.detail.value!.messages[0];
    const reused = invalidation === "none";
    assert.equal(
      message.content,
      reused ? "开头已经展开正文" : "开头",
      invalidation,
    );
    assert.equal(message.hasMoreContent, !reused, invalidation);
    assert.deepEqual(message.changes, reused ? changes : [], invalidation);
    assert.equal(
      message.metadata.status,
      "failed",
      "fresh metadata survives content reuse",
    );
    assert.equal(
      context.chunks.length,
      1,
      "navigation does not refetch expanded content",
    );
    context.reader.closeDetail();
  }
});

test("a new search keeps its native match offset instead of replacing it with earlier expanded content", async (t) => {
  const context = harness(t, {
    detail: async (request) => {
      const detail = detailWithLongMessage();
      if (request.query) {
        detail.detail.messages[0] = sessionMessage("long-message", "正文", {
          contentLength: 8,
          contentOffset: 6,
          hasMoreContent: false,
          contentVersion: 1,
        });
        detail.detail.find.query = request.query;
      }
      return detail;
    },
    message: async (request) => chunk(request, "已经展开正文", null),
  });
  await context.reader.openDetail("ref:session-1");
  await context.reader.expandMessage("long-message");
  await context.reader.navigateDetail({ query: "正文", matchPosition: 0 });
  assert.equal(context.reader.detail.value?.messages[0].content, "正文");
  assert.equal(context.reader.detail.value?.messages[0].contentOffset, 6);
});

test("reopening a reader does not treat its initial content as a previous expansion", async (t) => {
  let reads = 0;
  const context = harness(t, {
    detail: async () => {
      const detail = detailWithLongMessage();
      detail.detail.messages[0].content =
        ++reads === 2 ? "重新打开正文" : "开头";
      return detail;
    },
    message: async (request) => chunk(request, "已经展开正文", null),
  });
  await context.reader.openDetail("ref:session-1");
  await context.reader.expandMessage("long-message");
  context.reader.closeDetail();
  await context.reader.openDetail("ref:session-1");
  assert.equal(context.reader.detail.value?.messages[0].content, "重新打开正文");
  await context.reader.navigateDetail({ query: "开头" });
  assert.equal(context.reader.detail.value?.messages[0].content, "开头");
});

test("background indexing preserves expanded content only when its native message version is unchanged", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  for (const version of [1, 2]) {
    let reads = 0;
    const context = harness(t, {
      detail: async () => {
        const detail = detailWithLongMessage("text", reads ? version : 1);
        detail.detail.page.indexing = ++reads === 1;
        if (reads > 1) detail.detail.messages[0].metadata.status = "failed";
        return detail;
      },
      message: async (request) => chunk(request, "已经展开正文", null),
    });
    await context.reader.openDetail("ref:session-1");
    await context.reader.expandMessage("long-message");
    t.mock.timers.tick(250);
    await settle();
    const message = context.reader.detail.value!.messages[0];
    assert.equal(message.content, version === 1 ? "开头已经展开正文" : "开头");
    assert.equal(
      message.metadata.status,
      "failed",
      "new tool status survives content reuse",
    );
    context.reader.closeDetail();
  }
});

test("a newer indexed message cancels an in-flight expansion and releases its control immediately", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const pending = deferred<AgentSessionMessageContent>();
  let reads = 0;
  const context = harness(t, {
    detail: async () => {
      const detail = detailWithLongMessage("text", ++reads);
      detail.detail.page.indexing = reads === 1;
      return detail;
    },
    message: async () => pending.promise,
  });
  await context.reader.openDetail("ref:session-1");
  const expansion = context.reader.expandMessage("long-message");
  t.mock.timers.tick(250);
  await settle();
  assert.equal(context.reader.messageStates.value["long-message"], undefined);
  assert.ok(
    context.cancellations.some(
      (request) => request.consumerId === context.chunks[0].consumerId,
    ),
  );
  pending.resolve(chunk(context.chunks[0], "迟到的旧正文", null));
  await expansion;
  assert.equal(context.reader.detail.value?.messages[0].contentVersion, 2);
  assert.equal(context.reader.detail.value?.messages[0].content, "开头");
});
