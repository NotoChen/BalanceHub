import type { ProviderApiKeyOption } from "../stores/provider-types.ts";

export function effectiveProviderApiKeyOptions(
  configuredKey: string,
  options: ProviderApiKeyOption[],
) {
  const normalizedConfiguredKey = configuredKey.trim();
  const ordered: ProviderApiKeyOption[] = [];
  const seenLocalIds = new Set<string>();
  const seenTokenIds = new Set<string>();
  const seenKeys = new Set<string>();
  const seenMaskedKeys = new Set<string>();
  const add = (option: ProviderApiKeyOption) => {
    const localId = text(option.localId);
    const tokenId = text(option.tokenId);
    const key = text(option.key);
    const maskedKey = text(option.maskedKey);
    if (!localId && !tokenId && !key && !maskedKey) return;
    if (
      (localId && seenLocalIds.has(localId))
      || (tokenId && seenTokenIds.has(tokenId))
      || (key && seenKeys.has(key))
      || (!localId && !tokenId && !key && maskedKey && seenMaskedKeys.has(maskedKey))
    ) {
      return;
    }
    if (localId) seenLocalIds.add(localId);
    if (tokenId) seenTokenIds.add(tokenId);
    if (key) seenKeys.add(key);
    if (!localId && !tokenId && !key && maskedKey) seenMaskedKeys.add(maskedKey);
    ordered.push(option);
  };

  if (normalizedConfiguredKey) {
    const configured = options.find((option) => text(option.key) === normalizedConfiguredKey);
    add(configured ?? configuredProviderApiKeyOption(normalizedConfiguredKey));
  }
  for (const option of options) {
    if (normalizedConfiguredKey && text(option.key) === normalizedConfiguredKey) continue;
    add(option);
  }
  return ordered;
}

export function providerApiKeyOptionMatches(option: ProviderApiKeyOption, value: string) {
  const requested = value.trim();
  return Boolean(requested) && (
    text(option.localId) === requested
    || text(option.tokenId) === requested
    || text(option.key) === requested
  );
}

export function providerApiKeyOptionSelectionValue(option: ProviderApiKeyOption) {
  return text(option.localId) || text(option.tokenId) || text(option.key);
}

export function isProviderApiKeyUsable(option: ProviderApiKeyOption) {
  const key = text(option.key);
  return Boolean(option.keyAvailable && key && !key.includes("*"));
}

export function hasUsableProviderApiKey(
  configuredKey: string,
  options: ProviderApiKeyOption[],
) {
  const key = configuredKey.trim();
  return Boolean(key && !key.includes("*")) || options.some(isProviderApiKeyUsable);
}

function text(value: unknown) {
  return typeof value === "string" ? value.trim() : "";
}

function configuredProviderApiKeyOption(apiKey: string): ProviderApiKeyOption {
  return {
    localId: "",
    localName: "当前配置 API Key",
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
