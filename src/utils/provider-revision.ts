import type { Provider } from "../stores/provider-types.ts";

export type ProviderRevisionTombstones = Record<string, number>;

/// `activeEndpoint` 是进程内的备用地址读数，不随 provider revision 变化；
/// 同一 revision 的重复视图只有它可能不同，比较后决定是否替换对象。
export function sameActiveEndpoint(left: Provider, right: Provider) {
  const a = left.activeEndpoint;
  const b = right.activeEndpoint;
  if (a === b) return true;
  if (!a || !b) return false;
  return a.backupUrl === b.backupUrl
    && a.backupIndex === b.backupIndex
    && a.switchedAt === b.switchedAt;
}

export function mergeProvidersByRevision(
  currentProviders: Provider[],
  incomingProviders: Provider[],
  snapshotRevision: number,
  tombstones: ProviderRevisionTombstones,
) {
  if (incomingProviders.length === 0) return currentProviders;

  const incoming = new Map<string, Provider>();
  for (const provider of incomingProviders) {
    const id = provider.identity.id;
    const removedAt = tombstones[id] ?? 0;
    if (provider.revision <= removedAt) continue;

    const current = incoming.get(id);
    if (!current || provider.revision >= current.revision) {
      incoming.set(id, provider);
    }
  }

  // 没有真正更新的视图时保持原数组：数组身份不变，看板的筛选、分组和
  // 卡片渲染都不必为一次空响应重算。
  let changed = false;
  const next = currentProviders.map((current) => {
    const candidate = incoming.get(current.identity.id);
    if (!candidate) return current;
    incoming.delete(current.identity.id);
    if (candidate.revision < current.revision) return current;
    if (candidate.revision === current.revision && sameActiveEndpoint(candidate, current)) {
      return current;
    }
    changed = true;
    return candidate;
  });

  for (const candidate of incoming.values()) {
    if (candidate.revision >= snapshotRevision) {
      changed = true;
      next.push(candidate);
    }
  }
  return changed ? next : currentProviders;
}

export function pruneProviderTombstones(
  tombstones: ProviderRevisionTombstones,
  snapshotRevision: number,
) {
  return Object.fromEntries(
    Object.entries(tombstones).filter(([, removedAt]) => removedAt > snapshotRevision),
  );
}
