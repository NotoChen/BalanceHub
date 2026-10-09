import assert from "node:assert/strict";
import test, { type TestContext } from "node:test";
import { effectScope, ref } from "vue";
import { useAgentSessions } from "../src/composables/useAgentSessions.ts";
import type { AgentSessionQueryApi } from "../src/api/agent-sessions.ts";
import type { AgentSessionCancelRequest, AgentSessionDetail, AgentSessionDetailRequest, AgentSessionPage, AgentSessionParent, AgentSessionQuery } from "../src/stores/agent-session-types.ts";
import type { AgentCliKind } from "../src/stores/provider-types.ts";
import { sessionDetail, sessionPage, sessionRow, sessionScope } from "./agent-session-fixtures.ts";

function harness(t: TestContext, overrides: Partial<AgentSessionQueryApi> = {}, explicitPath?: string,
  configuration: Pick<Parameters<typeof useAgentSessions>[0], "pageSize" | "initialWorkspaceMode" | "refreshOnIndexUpdate" | "autoLoad" | "autoContinue"> = {}) {
  const queries: AgentSessionQuery[] = [];
  const details: AgentSessionDetailRequest[] = [];
  const cancellations: AgentSessionCancelRequest[] = [];
  const scopes: (string | null)[] = [];
  const active = ref(true);
  const agentKinds = ref<AgentCliKind[]>([]);
  const query = ref("");
  const scopeKey = ref("initial-recorded-directories");
  const explicitWorkdir = explicitPath === undefined ? undefined : ref<string | null>(explicitPath);
  const api: AgentSessionQueryApi = {
    scope: async (path = null) => { scopes.push(path); return overrides.scope ? overrides.scope(path) : sessionScope(path); },
    query: async (request) => {
      queries.push(request);
      return overrides.query ? overrides.query(request) : sessionPage([], { scopeRevision: request.scopeRevision });
    },
    detail: async (request) => {
      details.push(request);
      if (overrides.detail) return overrides.detail(request);
      throw new Error("unexpected detail read");
    },
    cancel: async (request) => { cancellations.push(request); await overrides.cancel?.(request); },
  };
  const lifetime = effectScope();
  const sessions = lifetime.run(() => useAgentSessions({ active, agentKinds, query, scopeKey, explicitWorkdir, autoLoad: false, ...configuration, api }))!;
  t.after(() => lifetime.stop());
  return { sessions, active, agentKinds, query, scopeKey, explicitWorkdir, queries, details, cancellations, scopes, dispose: () => lifetime.stop() };
}

test("default history searches all registered directories and an explicit selection only narrows that scope", async (t) => {
  const context = harness(t);
  await context.sessions.load();
  assert.deepEqual(context.scopes, [null]);
  assert.deepEqual(context.queries[0].workspaceIds, ["home", "project", "unmounted"]);
  assert.equal(context.queries[0].cursor, null);
  assert.equal(context.queries[0].pageSize, 50);
  assert.equal(context.queries[0].roleFilter, "all");
  assert.deepEqual(context.queries[0].agentKinds, []);
  context.sessions.selectWorkspace("project");
  await settle();
  assert.deepEqual(context.queries.at(-1)?.workspaceIds, ["project"]);
  context.sessions.selectWorkspace(null);
  await settle();
  assert.deepEqual(context.queries.at(-1)?.workspaceIds, ["home", "project", "unmounted"]);
  assert.equal(context.sessions.selectedWorkspaces.value.at(-1)?.exists, false);
});

test("completed and empty history pages survive repeated tab switches without another scope or query request", async (t) => {
  for (const rows of [[], [sessionRow("cached")]]) {
    const context = harness(t, { query: async () => sessionPage(rows, { nextCursor: rows.length ? "page:next" : null }) }, undefined, { autoLoad: true });
    await settle();
    const scope = context.sessions.scope.value;
    const snapshot = context.sessions.snapshotId.value;
    for (let visit = 0; visit < 3; visit += 1) {
      context.active.value = false;
      assert.deepEqual(context.sessions.rows.value, rows);
      assert.equal(context.sessions.scope.value, scope);
      assert.equal(context.sessions.busy.value, false);
      context.active.value = true;
      await settle();
      assert.equal(context.sessions.snapshotId.value, snapshot);
      assert.deepEqual(context.sessions.rows.value, rows);
    }
    assert.equal(context.queries.length, 1);
    assert.equal(context.scopes.length, 1);
    if (rows.length) {
      await context.sessions.loadMore();
      assert.equal(context.queries[1].cursor, "page:next", "ordinary pagination is retained but not fetched merely by returning");
    }
    context.dispose();
  }
});

test("inactive history keeps completed results, cancels a pending continuation and resumes from the same cursor", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const late = deferred<AgentSessionPage>();
  const first = sessionRow("first");
  const next = sessionRow("next");
  let reads = 0;
  const context = harness(t, { query: async () => {
    if (++reads === 1) return sessionPage([first], { scanPending: true, nextCursor: "scan:cached", total: null });
    if (reads === 2) return late.promise;
    return sessionPage([next], { loadedCount: 2, total: 2 });
  } }, undefined, { autoLoad: true });
  await settle();
  t.mock.timers.tick(150);
  await settle();
  assert.equal(context.sessions.loadingMore.value, true);
  context.active.value = false;
  assert.equal(context.sessions.busy.value, false);
  assert.equal(context.sessions.scanPending.value, true);
  assert.ok(context.cancellations.some((request) => request.requestId === context.queries[1].requestId));
  late.resolve(sessionPage([sessionRow("stale")], { scanPending: true, nextCursor: "scan:stale" }));
  await settle();
  t.mock.timers.tick(1000);
  await settle();
  assert.deepEqual(context.sessions.rows.value, [first]);
  assert.equal(context.queries.length, 2);
  context.active.value = true;
  t.mock.timers.tick(150);
  await settle();
  assert.deepEqual(context.queries.map((request) => request.cursor), [null, "scan:cached", "scan:cached"]);
  assert.deepEqual(context.sessions.rows.value.map((row) => row.session.id).sort(), ["first", "next"]);
  assert.equal(context.scopes.length, 1);
  assert.equal(context.sessions.scanPending.value, false);
});

test("a first query interrupted by navigation can restart once and never accept its late result", async (t) => {
  const late = deferred<AgentSessionPage>();
  let reads = 0;
  const context = harness(t, { query: async () => ++reads === 1 ? late.promise : sessionPage([sessionRow("current")]) }, undefined, { autoLoad: true });
  await settle();
  context.active.value = false;
  assert.equal(context.sessions.busy.value, false);
  context.active.value = true;
  await settle();
  late.resolve(sessionPage([sessionRow("stale")]));
  await settle();
  assert.deepEqual(context.sessions.rows.value.map((row) => row.session.id), ["current"]);
  assert.equal(context.queries.length, 2);
  assert.equal(context.scopes.length, 1);
});

test("real filters changed while history is hidden invalidate the cache and query only the final selection on return", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const context = harness(t, { query: async () => sessionPage([sessionRow()]) }, undefined, { autoLoad: true });
  await settle();
  context.active.value = false;
  context.agentKinds.value = ["claudeCode"];
  context.sessions.roleFilter.value = "subagent";
  context.sessions.selectWorkspace("project");
  context.query.value = "changed while hidden";
  t.mock.timers.tick(1000);
  await settle();
  assert.equal(context.queries.length, 1);
  assert.deepEqual(context.sessions.rows.value, []);
  context.active.value = true;
  await settle();
  assert.equal(context.queries.length, 2);
  assert.deepEqual(context.queries[1].agentKinds, ["claudeCode"]);
  assert.deepEqual(context.queries[1].workspaceIds, ["project"]);
  assert.equal(context.queries[1].roleFilter, "subagent");
  assert.equal(context.queries[1].query, "changed while hidden");
  assert.equal(context.scopes.length, 1);
});

test("cancelled and failed queries do not restart themselves on tab activation", async (t) => {
  for (const outcome of ["cancelled", "failed"]) {
    let reads = 0;
    const context = harness(t, { query: async () => {
      reads += 1;
      if (outcome === "failed" && reads === 1) throw new Error("source read failed");
      return sessionPage([sessionRow()], { scanPending: outcome === "cancelled", nextCursor: outcome === "cancelled" ? "scan:next" : null });
    } }, undefined, { autoLoad: true, autoContinue: false });
    await settle();
    if (outcome === "cancelled") context.sessions.cancel();
    context.active.value = false;
    context.active.value = true;
    await settle();
    assert.equal(context.queries.length, 1, outcome);
    assert.equal(context.sessions.busy.value, false, outcome);
    await context.sessions.refresh();
    assert.equal(context.queries.length, 2, "explicit refresh still starts a new query");
    context.dispose();
  }
});

test("cache-aware reads coalesce first loads and reuse the explicit directory after returning", async (t) => {
  const context = harness(t, {}, "/fixture/project");
  await Promise.all([context.sessions.ensureLoaded(), context.sessions.ensureLoaded()]);
  assert.equal(context.queries.length, 1);
  context.active.value = false;
  context.active.value = true;
  await context.sessions.ensureLoaded();
  assert.equal(context.queries.length, 1);
  context.explicitWorkdir!.value = "/fixture/other";
  await context.sessions.ensureLoaded();
  assert.equal(context.queries.length, 2);
  assert.deepEqual(context.scopes, ["/fixture/project", "/fixture/other"]);
});

test("pending scanning continues automatically, keeps visible results, and stops before ordinary pagination", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const continuation = deferred<AgentSessionPage>();
  const detailReply = deferred<AgentSessionDetail>();
  let reads = 0;
  const first = sessionRow("found-first");
  const second = sessionRow("found-later");
  const context = harness(t, {
    query: async () => ++reads === 1
      ? sessionPage([first], { total: null, nextCursor: "scan:1", scanPending: true })
      : continuation.promise,
    detail: async () => detailReply.promise,
  });
  await context.sessions.load();
  assert.equal(context.sessions.scanPending.value, true);
  assert.equal(context.sessions.busy.value, false);
  t.mock.timers.tick(150);
  await settle();
  assert.equal(context.sessions.loadingMore.value, true);
  assert.deepEqual(context.sessions.rows.value, [first]);
  const viewing = context.sessions.openDetail(first.sessionRef);
  context.sessions.closeDetail();
  assert.equal(context.sessions.detailVisible.value, false, "pending scanning and details never lock the modal");
  detailReply.resolve(sessionDetail(first));
  await viewing;
  continuation.resolve(sessionPage([second], { total: 3, loadedCount: 3, nextCursor: "page:2", scanPending: false }));
  await settle();
  assert.equal(context.sessions.rows.value.length, 2);
  assert.equal(context.sessions.scanPending.value, false);
  assert.equal(context.sessions.busy.value, false);
  t.mock.timers.tick(1000);
  await settle();
  assert.deepEqual(context.queries.map((request) => request.cursor), [null, "scan:1"]);
  assert.equal(context.sessions.hasMore.value, true, "the remaining ordinary page waits for user input");
});

test("scheduled continuation is cancelled by cancel, scope changes, inactivity and disposal", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  for (const action of ["cancel", "scope", "inactive", "dispose"]) {
    const context = harness(t, { query: async () => sessionPage([sessionRow()], { total: null, nextCursor: "scan:next", scanPending: true }) });
    await context.sessions.load();
    if (action === "cancel") context.sessions.cancel();
    if (action === "scope") context.scopeKey.value = "changed";
    if (action === "inactive") context.active.value = false;
    if (action === "dispose") context.dispose();
    t.mock.timers.tick(1000);
    await settle();
    assert.equal(context.queries.length, 1, action);
    assert.equal(context.sessions.scanPending.value, action === "inactive", action);
    assert.equal(context.sessions.busy.value, false, action);
    context.dispose();
  }
});

test("a failed or timed out continuation releases its state and never schedules another read", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  for (const failure of ["error", "timeout"]) {
    const late = deferred<AgentSessionPage>();
    let reads = 0;
    const context = harness(t, { query: async () => {
      if (++reads === 1) return sessionPage([sessionRow()], { nextCursor: "scan:next", scanPending: true, total: null });
      if (failure === "error") throw new Error("读取失败");
      return late.promise;
    } });
    await context.sessions.load();
    t.mock.timers.tick(150);
    await settle();
    if (failure === "timeout") { t.mock.timers.tick(65_000); await settle(); }
    assert.equal(context.sessions.busy.value, false);
    assert.equal(context.sessions.scanPending.value, false);
    assert.match(context.sessions.error.value, failure === "timeout" ? /超时/ : /读取失败/);
    assert.equal(context.sessions.rows.value.length, 1);
    late.resolve(sessionPage([sessionRow("late")], { nextCursor: "stale", scanPending: true }));
    await settle();
    t.mock.timers.tick(1000);
    await settle();
    assert.equal(context.queries.length, 2);
    assert.equal(context.sessions.rows.value.length, 1);
    context.dispose();
  }
});

test("a search change rejects the previous continuation and its timer", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const stale = deferred<AgentSessionPage>();
  let reads = 0;
  const current = sessionRow("current-query");
  const context = harness(t, { query: async () => {
    if (++reads === 1) return sessionPage([], { total: null, scanPending: true, nextCursor: "old-scan" });
    if (reads === 2) return stale.promise;
    return sessionPage([current], { snapshotId: "current-query" });
  } });
  await context.sessions.load();
  t.mock.timers.tick(150);
  await settle();
  context.query.value = "new query";
  t.mock.timers.tick(280);
  await settle();
  stale.resolve(sessionPage([sessionRow("stale-query")], { scanPending: true, nextCursor: "stale-next" }));
  await settle();
  t.mock.timers.tick(1000);
  await settle();
  assert.deepEqual(context.sessions.rows.value, [current]);
  assert.equal(context.sessions.scanPending.value, false);
  assert.equal(context.queries.length, 3);
  assert.equal(context.queries.at(-1)?.query, "new query");
});

test("count consumers start once with all backend workspace IDs and expose counts independently of the page", async (t) => {
  const counts = [
    { agentKind: "codex" as const, loadedCount: 155, total: 155 },
    { agentKind: "claudeCode" as const, loadedCount: 24, total: null },
  ];
  const context = harness(t, { query: async () => sessionPage([sessionRow()], {
    agentCounts: counts, loadedCount: 179, total: null, nextCursor: "snapshot:1:1",
    sourceStates: [{ sourceId: "source:codex", workspaceId: "home", state: "complete", loadedCount: 999, indexState: "ready", message: null }],
  }) }, undefined, { initialWorkspaceMode: "all", pageSize: 1, autoLoad: true, refreshOnIndexUpdate: false });
  await settle();
  assert.equal(context.queries.length, 1);
  assert.deepEqual(context.queries[0].workspaceIds, ["home", "project", "unmounted"]);
  assert.deepEqual(context.queries[0].agentKinds, []);
  assert.equal(context.queries[0].roleFilter, "all");
  assert.equal(context.queries[0].query, "");
  assert.equal(context.queries[0].pageSize, 1);
  assert.equal(context.queries[0].cursor, null);
  assert.equal(context.sessions.rows.value.length, 1);
  assert.deepEqual(context.sessions.agentCounts.value, counts);
  assert.equal(context.sessions.hasMore.value, true);
  await settle();
  assert.equal(context.queries.length, 1, "count consumers do not drain cursors automatically");
});

test("counts clear on refresh and scope invalidation and cancelled or late replies cannot restore them", async (t) => {
  const stale = deferred<AgentSessionPage>();
  const counts = [{ agentKind: "codex" as const, loadedCount: 155, total: 155 }];
  let reads = 0;
  const context = harness(t, { query: async () => ++reads === 1 ? sessionPage([], { agentCounts: counts }) : stale.promise }, undefined, { initialWorkspaceMode: "all", pageSize: 1 });
  await context.sessions.load();
  assert.deepEqual(context.sessions.agentCounts.value, counts);
  const refreshing = context.sessions.refresh();
  assert.deepEqual(context.sessions.agentCounts.value, []);
  await settle();
  context.sessions.cancel();
  assert.equal(context.sessions.busy.value, false);
  assert.equal(context.sessions.cancelled.value, true);
  assert.ok(context.cancellations.some((request) => request.requestId === context.queries[1].requestId));
  context.scopeKey.value = "recorded-path-forgotten";
  stale.resolve(sessionPage([], { agentCounts: counts }));
  await refreshing;
  assert.deepEqual(context.sessions.agentCounts.value, []);
  assert.equal(context.sessions.scope.value, null);
  assert.equal(context.sessions.busy.value, false);
});

test("the legacy picker uses the single canonical workspace returned for its explicit directory", async (t) => {
  const scope = sessionScope("/fixture/canonical/new-project");
  const context = harness(t, { scope: async () => scope }, "/fixture/alias");
  await context.sessions.load();
  assert.deepEqual(context.scopes, ["/fixture/alias"]);
  assert.deepEqual(context.queries[0].workspaceIds, ["workspace:/fixture/canonical/new-project"]);
  assert.equal(context.queries[0].scopeRevision, scope.revision);
  assert.equal(context.sessions.selectedWorkspaces.value.length, 1);
});

test("pagination loads more than 100 distinct sources, continues empty partial pages, and sorts only display rows", async (t) => {
  const rows = Array.from({ length: 125 }, (_, index) => sessionRow(`row-${String(index).padStart(3, "0")}`));
  rows[0].session.updatedAt = null;
  rows[124].session.updatedAt = "2026-09-17T01:00:00Z";
  const duplicate = structuredClone(rows[1]);
  duplicate.session.title = "更新后的合成标题";
  const pages = [
    sessionPage(rows.slice(0, 50), { total: null, nextCursor: "opaque:first" }),
    sessionPage([], { total: null, loadedCount: 50, nextCursor: "opaque:budget-continued", sourceStates: [{ sourceId: "source:codex", workspaceId: "home", state: "partial", loadedCount: 50, indexState: "fallback", message: "继续读取" }] }),
    sessionPage([duplicate, ...rows.slice(50, 99)], { total: null, loadedCount: 99, nextCursor: "opaque:last" }),
    sessionPage(rows.slice(99), { total: 125, loadedCount: 125 }),
  ];
  const context = harness(t, { query: async () => pages.shift()! });
  await context.sessions.load();
  await context.sessions.loadMore();
  assert.equal(context.sessions.rows.value.length, 50);
  assert.equal(context.sessions.total.value, null);
  assert.equal(context.sessions.incomplete.value, true);
  assert.equal(context.sessions.hasMore.value, true, "a cursor, not item count, determines continuation");
  await context.sessions.loadMore();
  await context.sessions.loadMore();
  assert.deepEqual(context.queries.map((request) => request.cursor), [null, "opaque:first", "opaque:budget-continued", "opaque:last"]);
  assert.equal(context.sessions.rows.value.length, 125);
  assert.equal(context.sessions.rows.value[0].sessionRef, rows[124].sessionRef);
  assert.deepEqual(context.sessions.rows.value.slice(1, -1).map((row) => row.sessionRef), rows.slice(1, 124).map((row) => row.sessionRef));
  assert.equal(context.sessions.rows.value.at(-1)?.sessionRef, rows[0].sessionRef);
  assert.equal(context.sessions.rows.value.find((row) => row.sessionRef === duplicate.sessionRef)?.session.title, duplicate.session.title);
  assert.equal(context.sessions.total.value, 125);
  assert.equal(context.sessions.hasMore.value, false);
});

test("parent updates correct loaded rows and open details without changing counts, cursor, or content", async (t) => {
  const unresolved: AgentSessionParent = { kind: "known", nativeId: "native-parent", parentRef: null };
  const linked: AgentSessionParent = { kind: "known", nativeId: "native-parent", parentRef: "ref:parent" };
  const child = sessionRow("child", { role: "subagent", parent: unresolved });
  const other = sessionRow("other");
  const pages = [
    sessionPage([child, other], { nextCursor: "opaque:parent", total: null }),
    sessionPage([], {
      parentUpdates: [
        { sessionRef: child.sessionRef, parent: linked },
        { sessionRef: "ref:unknown", parent: linked },
      ],
      loadedCount: 2, total: null, nextCursor: "opaque:ambiguity",
    }),
    sessionPage([], {
      parentUpdates: [{ sessionRef: child.sessionRef, parent: unresolved }],
      loadedCount: 2, total: 2,
    }),
  ];
  const context = harness(t, { query: async () => pages.shift()!, detail: async () => sessionDetail(child) });
  await context.sessions.load();
  await context.sessions.openDetail(child.sessionRef);
  const originalRows = structuredClone(context.sessions.rows.value);
  const originalDetail = context.sessions.detail.value;
  await context.sessions.loadMore();
  assert.deepEqual(context.sessions.rows.value.map((row) => row.sessionRef), originalRows.map((row) => row.sessionRef));
  assert.deepEqual(context.sessions.rows.value.find((row) => row.sessionRef === child.sessionRef), { ...child, parent: linked });
  assert.deepEqual(context.sessions.rows.value.find((row) => row.sessionRef === other.sessionRef), other);
  assert.equal(context.sessions.rows.value.length, 2, "an update for an unknown ref must not create a row");
  assert.equal(context.sessions.loadedCount.value, 2);
  assert.equal(context.sessions.nextCursor.value, "opaque:ambiguity");
  assert.deepEqual(context.sessions.detailRow.value?.parent, linked);
  assert.equal(context.sessions.detail.value, originalDetail, "parent correction must not replace detail content");
  await context.sessions.loadMore();
  assert.deepEqual(context.sessions.rows.value, originalRows, "new ambiguity withdraws only the resolved parent ref");
  assert.deepEqual(context.sessions.detailRow.value?.parent, unresolved);
  assert.equal(context.sessions.detail.value, originalDetail);
  assert.equal(context.sessions.loadedCount.value, 2);
  assert.equal(context.sessions.total.value, 2);
  assert.equal(context.sessions.nextCursor.value, null);
  assert.deepEqual(context.queries.map((request) => request.cursor), [null, "opaque:parent", "opaque:ambiguity"]);
});

test("parent updates from a late or mismatched scope cannot change the current loaded row", async (t) => {
  const late = deferred<AgentSessionPage>();
  const previous = sessionRow("same-ref", { role: "subagent", parent: { kind: "known", nativeId: "previous-parent", parentRef: null } });
  const current = sessionRow("same-ref", { role: "subagent", parent: { kind: "known", nativeId: "current-parent", parentRef: "ref:current-parent" } });
  const stalePage = sessionPage([], {
    parentUpdates: [{ sessionRef: current.sessionRef, parent: { kind: "known", nativeId: "previous-parent", parentRef: "ref:stale-parent" } }],
    loadedCount: 99, total: 99, nextCursor: "opaque:stale",
    agentCounts: [{ agentKind: "codex", loadedCount: 99, total: 99 }],
  });
  let currentScope = sessionScope();
  let reads = 0;
  const context = harness(t, {
    scope: async () => currentScope,
    query: async () => {
      reads += 1;
      if (reads === 1) return sessionPage([previous], { nextCursor: "opaque:old" });
      if (reads === 2) return late.promise;
      if (reads === 3) return sessionPage([current], { scopeRevision: currentScope.revision, snapshotId: "snapshot:current", nextCursor: "opaque:current",
        agentCounts: [{ agentKind: "codex", loadedCount: 1, total: 1 }] });
      return { ...stalePage, snapshotId: "snapshot:current" };
    },
  });
  await context.sessions.load();
  const pending = context.sessions.loadMore();
  await settle();
  currentScope = { ...sessionScope(), revision: "scope:current" };
  context.scopeKey.value = "changed-recorded-directories";
  await context.sessions.load();
  late.resolve(stalePage);
  await pending;
  assert.deepEqual(context.sessions.rows.value, [current]);
  assert.equal(context.sessions.loadedCount.value, 1);
  assert.equal(context.sessions.nextCursor.value, "opaque:current");
  assert.equal(context.sessions.snapshotId.value, "snapshot:current");
  assert.deepEqual(context.sessions.agentCounts.value, [{ agentKind: "codex", loadedCount: 1, total: 1 }]);
  await context.sessions.loadMore();
  assert.match(context.sessions.error.value, /会话范围或分页快照已变更/);
  assert.deepEqual(context.sessions.rows.value, [current]);
  assert.equal(context.sessions.loadedCount.value, 1);
  assert.equal(context.sessions.nextCursor.value, "opaque:current");
  assert.deepEqual(context.sessions.agentCounts.value, [{ agentKind: "codex", loadedCount: 1, total: 1 }]);
});

test("parent updates received while detail is pending survive its old row without blocking later detail reads", async (t) => {
  const unresolved: AgentSessionParent = { kind: "known", nativeId: "native-parent", parentRef: null };
  const linked: AgentSessionParent = { kind: "known", nativeId: "native-parent", parentRef: "ref:parent" };
  const transitions: [AgentSessionParent, AgentSessionParent][] = [[unresolved, linked], [linked, unresolved]];
  for (const [before, corrected] of transitions) {
    const child = sessionRow("pending-child", { role: "subagent", parent: before });
    const late = deferred<AgentSessionDetail>();
    const fresh = deferred<AgentSessionDetail>();
    const oldDetail = sessionDetail({ ...child, session: { ...child.session, title: "迟到详情的最新标题" } });
    oldDetail.detail.messages[0].content = "迟到的正文仍然应该显示";
    const laterDetail = sessionDetail({ ...child, parent: before });
    laterDetail.detail.messages[0].content = "后续主动读取的新正文";
    const pages = [
      sessionPage([child], { nextCursor: "opaque:parent-update", total: null }),
      sessionPage([], {
        parentUpdates: [{ sessionRef: child.sessionRef, parent: corrected }], loadedCount: 1, total: null, nextCursor: "opaque:parent-replay",
      }),
      sessionPage([], {
        parentUpdates: [{ sessionRef: child.sessionRef, parent: corrected }], loadedCount: 1, total: null, nextCursor: "opaque:parent-replay",
      }),
      sessionPage([], { parentUpdates: [{ sessionRef: child.sessionRef, parent: corrected }], loadedCount: 1, total: 1 }),
    ];
    let detailReads = 0;
    const context = harness(t, {
      query: async () => pages.shift()!,
      detail: async () => ++detailReads === 1 ? late.promise : fresh.promise,
    });
    await context.sessions.load();
    const pending = context.sessions.openDetail(child.sessionRef);
    await settle();
    assert.equal(context.sessions.detailLoading.value, true);
    await context.sessions.loadMore();
    assert.deepEqual(context.sessions.rows.value[0].parent, corrected);
    assert.equal(context.sessions.detail.value, null);
    late.resolve(oldDetail);
    await pending;
    assert.deepEqual(context.sessions.rows.value[0].parent, corrected);
    assert.deepEqual(context.sessions.detailRow.value?.parent, corrected);
    assert.equal(context.sessions.rows.value[0].session.title, oldDetail.row.session.title);
    assert.equal(context.sessions.detail.value, oldDetail.detail, "only the stale parent is replaced; body and detail fields remain usable");
    assert.equal(context.sessions.detailLoading.value, false);
    assert.equal(context.sessions.detailError.value, "");

    const activeDetail = context.sessions.openDetail(child.sessionRef);
    await settle();
    await context.sessions.loadMore();
    fresh.resolve(laterDetail);
    await activeDetail;
    assert.deepEqual(context.sessions.rows.value[0].parent, before, "a later read without an intervening update accepts its own parent");
    assert.deepEqual(context.sessions.detailRow.value?.parent, before);
    assert.equal(context.sessions.detail.value, laterDetail.detail);
    assert.equal(detailReads, 2);
    await context.sessions.loadMore();
    assert.deepEqual(context.sessions.rows.value[0].parent, before, "replaying the same parent patch must not overwrite a later detail relationship");
    assert.deepEqual(context.sessions.detailRow.value?.parent, before);
    assert.equal(context.sessions.detail.value, laterDetail.detail);
    assert.deepEqual(context.queries.slice(-2).map((request) => request.cursor), ["opaque:parent-replay", "opaque:parent-replay"]);
  }
});

test("independent list consumers and the detail consumer never cancel each other's reads", async (t) => {
  const listReply = deferred<AgentSessionPage>();
  const detailReply = deferred<AgentSessionDetail>();
  const first = harness(t, { query: async () => listReply.promise, detail: async () => detailReply.promise });
  const second = harness(t, { query: async () => sessionPage([sessionRow("other-surface")]) });
  const firstRead = first.sessions.load();
  await settle();
  await second.sessions.load();
  const detailRead = first.sessions.openDetail("ref:detail");
  first.sessions.invalidateList();
  assert.notEqual(first.queries[0].consumerId, second.queries[0].consumerId);
  assert.notEqual(first.details[0].consumerId, first.queries[0].consumerId);
  assert.deepEqual(first.cancellations, [{ consumerId: first.queries[0].consumerId, requestId: first.queries[0].requestId }]);
  assert.equal(first.sessions.detailLoading.value, true);
  assert.equal(second.cancellations.length, 0);
  detailReply.resolve(sessionDetail(sessionRow("detail")));
  await detailRead;
  listReply.resolve(sessionPage([sessionRow("obsolete")]));
  await firstRead;
  assert.equal(first.sessions.detail.value?.session.id, "detail");
  assert.deepEqual(first.sessions.rows.value, []);
  assert.equal(second.sessions.rows.value[0].session.id, "other-surface");
});

test("role and text search send backend filters and reject a late previous query", async (t) => {
  const obsolete = deferred<AgentSessionPage>();
  let reads = 0;
  const context = harness(t, { query: async (request) => ++reads === 1 ? obsolete.promise : sessionPage([sessionRow("new")], { scopeRevision: request.scopeRevision }) });
  const initial = context.sessions.load();
  await settle();
  context.agentKinds.value = ["claudeCode"];
  context.sessions.roleFilter.value = "subagent";
  await settle();
  assert.deepEqual(context.queries.at(-1)?.agentKinds, ["claudeCode"]);
  assert.equal(context.queries.at(-1)?.roleFilter, "subagent");
  await fastTimeout(280, async () => {
    context.query.value = "  合成正文命中  ";
    assert.deepEqual(context.sessions.rows.value, []);
    await tick();
    await settle();
  });
  assert.equal(context.queries.at(-1)?.query, "合成正文命中");
  assert.equal(context.queries.at(-1)?.cursor, null);
  obsolete.resolve(sessionPage([sessionRow("obsolete")]));
  await initial;
  assert.deepEqual(context.sessions.rows.value.map((row) => row.session.id), ["new"]);
  assert.ok(context.cancellations.some((request) => request.requestId === context.queries[0].requestId));
});

test("closing or changing recorded scope invalidates pending rows and details", async (t) => {
  const pageReply = deferred<AgentSessionPage>();
  const detailReply = deferred<AgentSessionDetail>();
  let reads = 0;
  const counts = [{ agentKind: "codex" as const, loadedCount: 155, total: 155 }];
  const context = harness(t, { query: async () => ++reads === 1 ? sessionPage([sessionRow()], { agentCounts: counts }) : pageReply.promise, detail: async () => detailReply.promise });
  await context.sessions.load();
  assert.deepEqual(context.sessions.agentCounts.value, counts);
  const readingDetail = context.sessions.openDetail("ref:session-1");
  const refreshing = context.sessions.refresh();
  await settle();
  context.scopeKey.value = "directory-forgotten";
  assert.equal(context.sessions.detailVisible.value, false);
  assert.equal(context.sessions.busy.value, false);
  context.active.value = false;
  pageReply.resolve(sessionPage([sessionRow("stale-scope")], { agentCounts: counts }));
  detailReply.resolve(sessionDetail());
  await Promise.all([refreshing, readingDetail]);
  assert.deepEqual(context.sessions.rows.value, []);
  assert.deepEqual(context.sessions.agentCounts.value, []);
  assert.equal(context.sessions.detail.value, null);
  assert.equal(context.sessions.scope.value, null);
});

test("query and detail timeouts release busy state, request cancellation, and ignore late data", async (t) => {
  const queryReply = deferred<AgentSessionPage>();
  const detailReply = deferred<AgentSessionDetail>();
  let reads = 0;
  const context = harness(t, { query: async () => ++reads === 1 ? queryReply.promise : sessionPage([sessionRow()]), detail: async () => detailReply.promise });
  await fastTimeout(65_000, () => context.sessions.load());
  assert.equal(context.sessions.busy.value, false);
  assert.match(context.sessions.error.value, /超时/);
  assert.ok(context.cancellations.some((request) => request.consumerId === context.queries[0].consumerId));
  queryReply.resolve(sessionPage([sessionRow("late")], { agentCounts: [{ agentKind: "codex", loadedCount: 999, total: 999 }] }));
  await settle();
  assert.deepEqual(context.sessions.rows.value, []);
  assert.deepEqual(context.sessions.agentCounts.value, []);
  await context.sessions.load();
  await fastTimeout(25_000, () => context.sessions.openDetail("ref:session-1"));
  assert.equal(context.sessions.detailLoading.value, false);
  assert.match(context.sessions.detailError.value, /超时/);
  assert.ok(context.cancellations.some((request) => request.consumerId === context.details[0].consumerId));
  context.sessions.closeDetail();
  detailReply.resolve(sessionDetail());
  await settle();
  assert.equal(context.sessions.detailVisible.value, false);
  assert.equal(context.sessions.detail.value, null);
});

test("same native IDs remain distinct by source ref and detail rejects the wrong source", async (t) => {
  const first = sessionRow("shared-id", { sessionRef: "ref:source-a" });
  const other = sessionRow("shared-id", { sessionRef: "ref:source-b", sourceId: "source:other" });
  const context = harness(t, { query: async () => sessionPage([other, first]), detail: async () => sessionDetail(other) });
  await context.sessions.load();
  assert.deepEqual(context.sessions.rows.value.map((row) => row.sessionRef), ["ref:source-a", "ref:source-b"]);
  await context.sessions.openDetail(first.sessionRef);
  assert.match(context.sessions.detailError.value, /不同的原生来源/);
  assert.equal(context.sessions.detail.value, null);
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}
async function settle() { for (let index = 0; index < 16; index += 1) await Promise.resolve(); }
const originalTimeout = globalThis.setTimeout;
function tick() { return new Promise<void>((resolve) => originalTimeout(resolve, 0)); }
async function fastTimeout<T>(milliseconds: number, action: () => Promise<T>) {
  globalThis.setTimeout = ((callback: (...args: unknown[]) => void, delay?: number, ...args: unknown[]) =>
    originalTimeout(callback, delay === milliseconds ? 0 : delay, ...args)) as typeof setTimeout;
  try { return await action(); } finally { globalThis.setTimeout = originalTimeout; }
}
