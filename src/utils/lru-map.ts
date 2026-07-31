export function setLruEntry<K, V>(cache: Map<K, V>, key: K, value: V, capacity: number) {
  if (capacity < 1) {
    cache.clear();
    return;
  }
  cache.delete(key);
  cache.set(key, value);
  while (cache.size > capacity) {
    const oldest = cache.keys().next();
    if (oldest.done) break;
    cache.delete(oldest.value);
  }
}

export function touchLruEntry<K, V>(cache: Map<K, V>, key: K): V | undefined {
  const value = cache.get(key);
  if (value === undefined) return undefined;
  cache.delete(key);
  cache.set(key, value);
  return value;
}

export function pruneLruEntries<K, V>(cache: Map<K, V>, keep: (key: K) => boolean) {
  for (const key of cache.keys()) {
    if (!keep(key)) {
      cache.delete(key);
    }
  }
}
