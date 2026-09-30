import type { Provider } from "../stores/provider-types";
import {
  providerCardStatusTone,
  providerHasNoAvailableBalance,
  providerProtocolLabel,
} from "./provider-display.ts";

export const providerFilters = [
  { value: "all", label: "全部", description: "全部中转站" },
  { value: "checkIn", label: "待签到", description: "尚未完成今日签到" },
  { value: "attention", label: "需关注", description: "异常、待同步或无余额" },
  {
    value: "newApi",
    label: providerProtocolLabel("newApi"),
    description: "NewAPI 中转站，包含 AnyRouter",
  },
  {
    value: "sub2Api",
    label: providerProtocolLabel("sub2Api"),
    description: "Sub2API 中转站",
  },
  { value: "api", label: "API Key", description: "通用 API Key 中转站" },
] as const;

export type ProviderFilter = (typeof providerFilters)[number]["value"];
export type ProviderFilterCounts = Record<ProviderFilter, number>;

export function providerMatchesFilter(
  provider: Provider,
  filter: ProviderFilter,
) {
  switch (filter) {
    case "all":
      return true;
    case "newApi":
    case "sub2Api":
    case "api":
      return provider.identity.protocol === filter;
    case "checkIn":
      return providerCardStatusTone(provider) === "warning";
    case "attention": {
      const tone = providerCardStatusTone(provider);
      // 待签到和需关注可以重叠，不能让卡片的签到提示掩盖无余额。
      return (
        tone !== "disabled" &&
        (tone === "error" ||
          tone === "pending" ||
          providerHasNoAvailableBalance(provider))
      );
    }
  }
}

export function countProviderFilters(
  providers: Provider[],
): ProviderFilterCounts {
  const counts: ProviderFilterCounts = {
    all: providers.length,
    checkIn: 0,
    attention: 0,
    newApi: 0,
    sub2Api: 0,
    api: 0,
  };
  for (const provider of providers) {
    for (const filter of providerFilters) {
      if (
        filter.value !== "all" &&
        providerMatchesFilter(provider, filter.value)
      ) {
        counts[filter.value]++;
      }
    }
  }
  return counts;
}

/** Search only user-visible provider metadata; credentials are intentionally excluded. */
export function providerMatchesSearch(provider: Provider, query: string) {
  const terms = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
  if (terms.length === 0) {
    return true;
  }

  const searchableFields = [
    provider.identity.name,
    provider.identity.remark,
    provider.identity.displayName,
    provider.identity.baseUrl,
    ...provider.identity.backupUrls,
    provider.identity.username,
    provider.identity.userId,
    provider.auth.apiUser,
    ...provider.auth.apiKeyOptions.flatMap((option) => [
      option.localName,
      option.name,
    ]),
    provider.cli.preferredModel,
    provider.liveness.model,
    ...Object.values(provider.liveness.agentBaseUrls || {}),
    ...provider.capabilities.availableModels,
    ...provider.liveness.records.map((record) => record.model),
  ]
    .map((value) => value?.trim().toLocaleLowerCase())
    .filter((value): value is string => Boolean(value));

  return terms.every((term) =>
    searchableFields.some((field) => field.includes(term)),
  );
}
