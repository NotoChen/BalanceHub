import type { Provider } from "../stores/provider-types.ts";

export type ProviderRevisionTombstones = Record<string, number>;

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

  const next = currentProviders.map((current) => {
    const candidate = incoming.get(current.identity.id);
    if (!candidate) return current;
    incoming.delete(current.identity.id);
    return candidate.revision < current.revision ? current : candidate;
  });

  for (const candidate of incoming.values()) {
    if (candidate.revision >= snapshotRevision) {
      next.push(candidate);
    }
  }
  return next;
}

export function pruneProviderTombstones(
  tombstones: ProviderRevisionTombstones,
  snapshotRevision: number,
) {
  return Object.fromEntries(
    Object.entries(tombstones).filter(([, removedAt]) => removedAt > snapshotRevision),
  );
}
