import { shallowReactive } from "vue";
import type { AgentCliKind } from "../stores/provider-types";
import { withTimeout } from "./promise-timeout";

export interface AgentPublicationEntry<T> {
  value: T | null;
  loading: boolean;
  error: string;
  stale: boolean;
}
export type AgentPublicationLoader<T> = (workspace: string | null | undefined) => Promise<T>;

/** Native source lists replace actor-bound access references.
 * Serialize publication and consumption by Agent, including lexical workspace aliases.
 * Holds defer refreshes; they never disable a page or retain editable content. */
export function createAgentPublicationCache<T extends { agentKind: AgentCliKind }>(options: {
  load: (kind: AgentCliKind, workspace: string | null) => Promise<T>;
  timeoutMs: number;
  error: (failure: unknown) => string;
}) {
  const entries = new Map<string, { kind: AgentCliKind; workspace: string | null; state: AgentPublicationEntry<T>; subscribers: number }>();
  const tails = new Map<AgentCliKind, Promise<void>>();
  const pending = new Map<string, Promise<T | null>>();
  const holds = new Map<AgentCliKind, Set<symbol>>();
  const deferred = new Set<string>();
  const revisions = new Map<string, number>();
  let sequence = 0;
  let disposed = false;
  const keyFor = (kind: AgentCliKind, workspace: string | null | undefined) => JSON.stringify([kind, workspace || null]);
  function record(kind: AgentCliKind, workspace: string | null | undefined) {
    const key = keyFor(kind, workspace);
    let entry = entries.get(key);
    if (!entry) {
      entry = { kind, workspace: workspace || null, subscribers: 0,
        state: shallowReactive<AgentPublicationEntry<T>>({ value: null, loading: false, error: "", stale: true }) };
      entries.set(key, entry);
      // Only inactive metadata is evicted; open views and in-flight work retain their entry.
      for (const [oldKey, old] of entries) {
        if (entries.size <= 32) break;
        if (oldKey !== key && !old.subscribers && !pending.has(oldKey) && !old.state.loading) {
          entries.delete(oldKey); revisions.delete(oldKey); deferred.delete(oldKey);
        }
      }
    }
    return entry;
  }
  function get(kind: AgentCliKind, workspace?: string | null) { return record(kind, workspace).state; }

  function transaction<R>(kind: AgentCliKind, work: (publish: AgentPublicationLoader<T>) => Promise<R>, timeoutMs = 60_000, signal?: AbortSignal): Promise<R> {
    let abandoned = false;
    const owned = new Map<string, { request: number; entry: AgentPublicationEntry<T> }>();
    const current = () => !disposed && !abandoned && !signal?.aborted;
    const assertCurrent = () => { if (!current()) throw new Error("来源读取已取消"); };
    const publish: AgentPublicationLoader<T> = async (workspace) => {
      assertCurrent();
      const entry = record(kind, workspace).state;
      const key = keyFor(kind, workspace);
      const request = ++sequence;
      revisions.set(key, request);
      owned.set(key, { request, entry });
      entry.loading = true;
      entry.error = "";
      try {
        const result = await withTimeout(options.load(kind, workspace || null), options.timeoutMs, "读取原生来源超时");
        assertCurrent();
        if (revisions.get(key) !== request) throw new Error("来源请求已过期");
        if (result.agentKind !== kind) throw new Error("来源环境不一致");
        entry.value = result;
        entry.stale = false;
        return result;
      } catch (failure) {
        if (current() && revisions.get(key) === request) { entry.error = options.error(failure); entry.stale = true; }
        throw failure;
      } finally { if (revisions.get(key) === request) entry.loading = false; }
    };
    const previous = tails.get(kind) ?? Promise.resolve();
    const workPromise = previous.then(() => { assertCurrent(); return work(publish); });
    let onAbort: (() => void) | undefined;
    const cancellable = signal ? Promise.race([workPromise, new Promise<never>((_resolve, reject) => {
      onAbort = () => reject(new Error("来源操作已取消"));
      signal.addEventListener("abort", onAbort, { once: true });
      if (signal.aborted) onAbort();
    })]) : workPromise;
    const timed = withTimeout(cancellable, timeoutMs, "等待原生来源操作超时").finally(() => {
      abandoned = true;
      if (onAbort) signal?.removeEventListener("abort", onAbort);
      for (const [key, { request, entry }] of owned) {
        if (revisions.get(key) === request && entry.loading) {
          entry.loading = false;
          entry.stale = true;
        }
      }
    });
    // Cancelling work that has not started must not let later work jump its predecessor.
    const tail = Promise.all([previous, timed.then(() => {}, () => {})]).then(() => {});
    tails.set(kind, tail);
    void tail.then(() => { if (tails.get(kind) === tail) tails.delete(kind); });
    return timed;
  }

  function refresh(kind: AgentCliKind, workspace?: string | null): Promise<T | null> {
    const key = keyFor(kind, workspace);
    const entry = get(kind, workspace);
    if (disposed) return Promise.resolve(entry.value);
    if (holds.get(kind)?.size) { entry.stale = true; deferred.add(key); return Promise.resolve(entry.value); }
    const existing = pending.get(key);
    if (existing) return existing;
    const request = transaction(kind, async (publish) => {
      if (holds.get(kind)?.size) { entry.stale = true; deferred.add(key); return entry.value; }
      return publish(workspace);
    }, options.timeoutMs + 5000).catch((failure) => {
      if (!disposed) { entry.error = options.error(failure); entry.stale = true; }
      return null;
    }).finally(() => { if (pending.get(key) === request) pending.delete(key); });
    pending.set(key, request);
    return request;
  }
  function subscribe(kind: AgentCliKind, workspace?: string | null) {
    const key = keyFor(kind, workspace);
    const entry = record(kind, workspace);
    entry.subscribers += 1;
    if (!entry.state.value || entry.state.stale) void refresh(kind, workspace);
    let released = false;
    return () => { if (!released) { released = true; entry.subscribers -= 1; if (!entry.subscribers) deferred.delete(key); } };
  }
  function hold(kind: AgentCliKind) {
    const token = Symbol(kind);
    const owned = holds.get(kind) ?? new Set<symbol>();
    owned.add(token);
    holds.set(kind, owned);
    return () => {
      if (!owned.delete(token) || owned.size || disposed) return;
      holds.delete(kind);
      for (const key of [...deferred]) {
        const entry = entries.get(key);
        if (entry?.kind !== kind) continue;
        deferred.delete(key);
        void refresh(kind, entry.workspace);
      }
    };
  }
  function invalidate(kind: AgentCliKind) {
    for (const entry of entries.values()) {
      if (entry.kind !== kind) continue;
      entry.state.stale = true;
      if (entry.subscribers) void refresh(kind, entry.workspace);
    }
  }
  function dispose() {
    disposed = true;
    for (const entry of entries.values()) entry.state.loading = false;
    deferred.clear(); holds.clear(); pending.clear(); entries.clear(); revisions.clear();
  }
  return { get, refresh, subscribe, hold, invalidate, transaction, dispose };
}
