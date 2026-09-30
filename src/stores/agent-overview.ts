import { defineStore } from "pinia";
import { ref, shallowRef } from "vue";
import { getCachedAgentOverview, refreshAgentOverview } from "../api/agent-overview";
import { withTimeout } from "../utils/promise-timeout";
import { agentEnvironmentErrorMessage, agentEnvironmentKey } from "./agent-environment";
import type { AgentCliKind } from "./provider-types";
import type { AgentOverviewSnapshot } from "./agent-overview-types";
import { useCliRuntimeStore } from "./cli-runtime";
import { useSettingsStore } from "./settings";

export const useAgentOverviewStore = defineStore("agent-overview", () => {
  const snapshots = shallowRef<Record<string, Partial<Record<AgentCliKind, AgentOverviewSnapshot>>>>({});
  const refreshing = ref<Record<string, boolean>>({});
  const rescanning = ref<Record<string, boolean>>({});
  const errors = ref<Record<string, string>>({});
  const restored = new Map<string, Promise<void>>();
  const pending = new Map<string, Promise<AgentOverviewSnapshot | null>>();
  const dirty = new Set<string>();
  const stale = new Set<string>();
  const invalidationRevision = ref(0);
  const revisions = new Map<string, number>();
  const key = (kind: AgentCliKind, workspace?: string) => JSON.stringify([agentEnvironmentKey(workspace), kind]);
  const get = (kind: AgentCliKind, workspace?: string) => snapshots.value[agentEnvironmentKey(workspace)]?.[kind] ?? null;

  function publish(snapshot: AgentOverviewSnapshot, workspace?: string) {
    const scope = agentEnvironmentKey(workspace);
    snapshots.value = { ...snapshots.value, [scope]: { ...snapshots.value[scope], [snapshot.agentKind]: snapshot } };
  }
  function restore(workspace?: string): Promise<void> {
    const scope = agentEnvironmentKey(workspace);
    const existing = restored.get(scope);
    if (existing) return existing;
    if (!snapshots.value[scope]) snapshots.value = { ...snapshots.value, [scope]: {} };
    const task = withTimeout(getCachedAgentOverview(workspace), 10_000, "读取 Agent 摘要超时")
      .then((cached) => { for (const snapshot of cached) if (!get(snapshot.agentKind, workspace)) publish(snapshot, workspace); })
      .catch(() => { restored.delete(scope); });
    restored.set(scope, task);
    return task;
  }
  function refresh(kind: AgentCliKind, workspace?: string, force = false): Promise<AgentOverviewSnapshot | null> {
    const id = key(kind, workspace);
    const existing = pending.get(id);
    if (existing) {
      if (force) { dirty.add(id); rescanning.value[id] = true; }
      return existing;
    }
    force = force || stale.has(id);
    const revision = (revisions.get(id) ?? 0) + 1;
    revisions.set(id, revision);
    refreshing.value[id] = true;
    rescanning.value[id] = force;
    delete errors.value[id];
    const task = (async () => {
      let forceScan = force;
      let snapshot: AgentOverviewSnapshot;
      do {
        dirty.delete(id);
        const selectedPath = useSettingsStore().settings.agentCliPaths[kind];
        snapshot = await withTimeout(refreshAgentOverview(kind, workspace, forceScan), 60_000, "刷新 Agent 摘要超时");
        if (selectedPath !== useSettingsStore().settings.agentCliPaths[kind]) dirty.add(id);
        forceScan = dirty.has(id);
        if (!forceScan && revisions.get(id) === revision) {
          stale.delete(id);
          publish(snapshot, workspace);
          useCliRuntimeStore().acceptCliToolProbe(snapshot.probe);
        }
      } while (forceScan);
      return snapshot;
    })()
      .catch((error) => { if (revisions.get(id) === revision) errors.value[id] = agentEnvironmentErrorMessage(error); return null; })
      .finally(() => { if (pending.get(id) === task) pending.delete(id); if (revisions.get(id) === revision) { refreshing.value[id] = false; rescanning.value[id] = false; } });
    pending.set(id, task);
    return task;
  }
  async function check(kinds: AgentCliKind[], workspace?: string, force = false) {
    await restore(workspace);
    await Promise.allSettled(kinds.map((kind) => refresh(kind, workspace, force)));
  }
  function invalidate(kind: AgentCliKind) {
    invalidationRevision.value += 1;
    for (const scope of Object.keys(snapshots.value)) {
      const id = key(kind, scope === "__native__" ? undefined : scope);
      stale.add(id);
      if (pending.has(id)) dirty.add(id);
    }
  }
  return { invalidationRevision, snapshots, refreshing, rescanning, errors, key, get, restore, refresh, check, invalidate };
});
