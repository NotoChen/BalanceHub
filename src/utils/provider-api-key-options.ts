import type { ProviderApiKeyOption } from "../stores/provider-types.ts";

export function effectiveProviderApiKeyOptions(
  configuredKey: string,
  remoteOptions: ProviderApiKeyOption[],
) {
  const normalizedConfiguredKey = configuredKey.trim();
  const options: ProviderApiKeyOption[] = [];
  if (normalizedConfiguredKey) {
    options.push(configuredProviderApiKeyOption(normalizedConfiguredKey));
  }

  const knownKeys = new Set([normalizedConfiguredKey]);
  for (const option of remoteOptions) {
    const key = option.key.trim();
    if (!key || knownKeys.has(key)) continue;
    knownKeys.add(key);
    options.push(option);
  }
  return options;
}

function configuredProviderApiKeyOption(apiKey: string): ProviderApiKeyOption {
  return {
    name: "当前配置 API Key",
    key: apiKey,
    maskedKey: "",
    keyAvailable: true,
    tokenId: "",
    userId: "",
    status: "enabled",
    usedQuota: 0,
    remainQuota: 0,
    usedQuotaRaw: 0,
    remainQuotaRaw: 0,
    unlimitedQuota: false,
    group: "",
    crossGroupRetry: false,
    modelLimitsEnabled: false,
    modelLimits: [],
    allowIps: [],
    quotaDisplayType: "currency",
    currencySymbol: "$",
  };
}
