import type {
  ProviderApiKeyOption,
  ProviderCapabilityProbeResult,
  ProviderConnectionTestResult,
  ProviderInput,
  ProviderSaveOptions,
  ProviderSaveResult,
  ProviderProtocolDetectionResult,
  ProviderProtocolDescriptor,
  ProviderSaveConflict,
  ProviderSiteProbeResult,
} from "../stores/providers";

export interface ProviderEditorStore {
  providerProtocols: ProviderProtocolDescriptor[];
  saveProvider: (input: ProviderInput, options?: ProviderSaveOptions) => Promise<ProviderSaveResult>;
  detectProviderProtocol: (input: ProviderInput) => Promise<ProviderProtocolDetectionResult>;
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

export type ProtocolSelectionSource = "auto" | "unresolved" | "manual" | "saved";
export type ProviderEditorStep = "basics" | "credentials" | "advanced";
export type ProviderDuplicateDecision = "createSeparate" | "merge" | "overwrite" | "cancel";
export type ProviderSaveCompletion = "standard" | "mergedApiKey";

export function providerDuplicateSaveResolution(
  conflict: ProviderSaveConflict,
  decision: ProviderDuplicateDecision,
): { options: ProviderSaveOptions; completion: ProviderSaveCompletion } | null {
  if (decision === "cancel") return null;

  if (conflict.kind === "sameUrlDifferentApiKey") {
    if (decision === "merge") {
      return {
        options: { mergeApiKeyIntoProviderId: conflict.existingProviderId },
        completion: "mergedApiKey",
      };
    }
    if (decision === "createSeparate") {
      return {
        options: { createSeparateFromProviderId: conflict.existingProviderId },
        completion: "standard",
      };
    }
    return null;
  }

  if (decision !== "overwrite") return null;
  return {
    options: { overwriteProviderId: conflict.existingProviderId },
    completion: "standard",
  };
}

export function normalizeProviderBaseUrl(value: string) {
  return value.trim().replace(/\/+$/, "").toLowerCase();
}

export function fieldLabel(field: string) {
  const labels: Record<string, string> = {
    accessToken: "访问令牌",
    refreshToken: "刷新令牌",
    accessTokenExpiresAt: "访问令牌有效期",
    apiKey: "API 密钥",
    apiKeyTokenId: "主 API Key",
    apiKeyOptions: "API Key 列表",
    apiUser: "API User ID",
    loginUsername: "登录账号",
  };
  return labels[field] ?? field;
}
