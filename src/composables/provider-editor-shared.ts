import type {
  Provider,
  ProviderApiKeyOption,
  ProviderCapabilityProbeResult,
  ProviderConnectionTestResult,
  ProviderInput,
  ProviderSiteProbeResult,
} from "../stores/providers";

export interface ProviderEditorStore {
  saveProvider: (input: ProviderInput) => Promise<Provider[]>;
  probeProviderSite: (input: ProviderInput) => Promise<ProviderSiteProbeResult>;
  completeProviderCredentials: (input: ProviderInput) => Promise<{
    input: ProviderInput;
    changedFields: string[];
    steps: { name: string; ok: boolean; message: string }[];
    apiKeyOptions: ProviderApiKeyOption[];
  }>;
  testProviderConnection: (input: ProviderInput) => Promise<ProviderConnectionTestResult>;
  createApiKeyForInput: (input: ProviderInput, name: string) => Promise<ProviderApiKeyOption>;
  generateAccessTokenForInput: (input: ProviderInput) => Promise<string>;
  refreshByIds: (ids: string[]) => Promise<unknown>;
  probeCapabilities: (id: string) => Promise<ProviderCapabilityProbeResult>;
}

export function normalizeProviderBaseUrl(value: string) {
  return value.trim().replace(/\/+$/, "").toLowerCase();
}

export function findSavedProvider(savedProviders: Provider[], input: ProviderInput) {
  if (input.id) {
    const provider = savedProviders.find((item) => item.identity.id === input.id);
    if (provider) {
      return provider;
    }
  }

  const baseUrl = normalizeProviderBaseUrl(input.identity.baseUrl);
  return [...savedProviders].reverse().find((provider) => normalizeProviderBaseUrl(provider.identity.baseUrl) === baseUrl);
}

export function fieldLabel(field: string) {
  const labels: Record<string, string> = {
    accessToken: "访问令牌",
    apiKey: "API 密钥",
    apiKeyTokenId: "主 API Key",
    apiKeyOptions: "API Key 列表",
    apiUser: "API User ID",
    loginUsername: "登录账号",
  };
  return labels[field] ?? field;
}
