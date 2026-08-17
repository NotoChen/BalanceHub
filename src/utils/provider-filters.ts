import type { Provider } from "../stores/provider-types";

/** Search only user-visible provider metadata; credentials are intentionally excluded. */
export function providerMatchesSearch(provider: Provider, query: string) {
  const terms = query
    .trim()
    .toLocaleLowerCase()
    .split(/\s+/)
    .filter(Boolean);
  if (terms.length === 0) {
    return true;
  }

  const searchableFields = [
    provider.identity.name,
    provider.identity.displayName,
    provider.identity.baseUrl,
    ...provider.identity.backupUrls,
    provider.identity.username,
    provider.identity.userId,
    provider.auth.apiUser,
    provider.cli.preferredModel,
    provider.liveness.model,
    ...Object.values(provider.liveness.agentBaseUrls || {}),
    ...provider.capabilities.availableModels,
    ...provider.liveness.records.map((record) => record.model),
  ]
    .map((value) => value?.trim().toLocaleLowerCase())
    .filter((value): value is string => Boolean(value));

  return terms.every((term) => searchableFields.some((field) => field.includes(term)));
}
