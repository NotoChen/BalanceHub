import type { Provider } from "../stores/providers";

export type CcSwitchAppTarget =
  | "codex"
  | "claude"
  | "opencode"
  | "openclaw"
  | "hermes";

const OPENAI_TARGETS = new Set<CcSwitchAppTarget>([
  "codex",
  "opencode",
  "openclaw",
  "hermes",
]);

export const ccSwitchTargetLabels: Record<CcSwitchAppTarget, string> = {
  codex: "Codex",
  claude: "Claude Code",
  opencode: "OpenCode",
  openclaw: "OpenClaw",
  hermes: "Hermes",
};

export const ccSwitchTargets: CcSwitchAppTarget[] = [
  "codex",
  "claude",
  "opencode",
  "openclaw",
  "hermes",
];

export function canBuildCcSwitchDeeplink(provider: Provider) {
  return Boolean(provider.identity.baseUrl.trim() && provider.auth.apiKey.trim());
}

export function buildCcSwitchProviderDeeplink(
  provider: Provider,
  target: CcSwitchAppTarget,
) {
  const endpoint = endpointForTarget(provider, target);
  const params = new URLSearchParams();
  params.set("resource", "provider");
  params.set("app", target);
  params.set("name", provider.identity.name.trim() || provider.identity.displayName.trim() || "BalanceHub");
  params.set("homepage", normalizeOrigin(provider.identity.baseUrl));
  params.set("endpoint", endpoint);
  params.set("apiKey", provider.auth.apiKey.trim());
  params.set("icon", iconForTarget(target));
  params.set("notes", "由 BalanceHub 导入");
  params.set("enabled", "false");

  const model = preferredModel(provider);
  if (model) {
    params.set("model", model);
  }

  appendUsageScriptParams(params, provider);

  return `ccswitch://v1/import?${params.toString()}`;
}

export function ccSwitchEndpointHint(provider: Provider, target: CcSwitchAppTarget) {
  return endpointForTarget(provider, target);
}

function iconForTarget(target: CcSwitchAppTarget) {
  if (target === "codex") {
    return "openai";
  }
  return target;
}

function endpointForTarget(provider: Provider, target: CcSwitchAppTarget) {
  if (target === "claude") {
    return anthropicBaseUrl(provider);
  }
  if (OPENAI_TARGETS.has(target)) {
    return openaiBaseUrl(provider);
  }
  return normalizeUrl(provider.identity.baseUrl);
}

function openaiBaseUrl(provider: Provider) {
  const raw = provider.liveness.openaiBaseUrl.trim() || provider.identity.baseUrl.trim();
  const normalized = normalizeUrl(raw);
  return normalized.endsWith("/v1") ? normalized : `${normalized}/v1`;
}

function anthropicBaseUrl(provider: Provider) {
  const raw = provider.liveness.anthropicBaseUrl.trim() || provider.identity.baseUrl.trim();
  return normalizeUrl(raw);
}

function normalizeUrl(value: string) {
  return value.trim().replace(/\/+$/, "");
}

function normalizeOrigin(value: string) {
  const normalized = normalizeUrl(value);
  try {
    const url = new URL(normalized);
    return `${url.protocol}//${url.host}`;
  } catch {
    return normalized;
  }
}

function preferredModel(provider: Provider) {
  const livenessModel = provider.liveness.model.trim();
  if (livenessModel) {
    return livenessModel;
  }
  return (provider.capabilities.availableModels || [])
    .map((model) => model.trim())
    .find(Boolean);
}

function appendUsageScriptParams(params: URLSearchParams, provider: Provider) {
  const accessToken = provider.auth.accessToken.trim();
  const userId = provider.auth.apiUser.trim();
  if (!accessToken || !userId) {
    return;
  }

  params.set("usageEnabled", "true");
  params.set("usageScript", base64Ascii(newApiUsageScript()));
  params.set("usageBaseUrl", normalizeUrl(provider.identity.baseUrl));
  params.set("usageAccessToken", accessToken);
  params.set("usageUserId", userId);

  const intervalMinutes = Math.max(1, Math.round((provider.automation.refreshInterval || 300) / 60));
  params.set("usageAutoInterval", String(intervalMinutes));
}

function base64Ascii(value: string) {
  return btoa(value);
}

function newApiUsageScript() {
  return `({
  request: {
    url: "{{baseUrl}}/api/user/self",
    method: "GET",
    headers: {
      "Content-Type": "application/json",
      "Authorization": "Bearer {{accessToken}}",
      "User-Agent": "cc-switch/1.0",
      "New-Api-User": "{{userId}}"
    }
  },
  extractor: function(response) {
    if (response.success && response.data) {
      return {
        planName: response.data.group || "Default",
        remaining: response.data.quota / 500000,
        used: response.data.used_quota / 500000,
        total: (response.data.quota + response.data.used_quota) / 500000,
        unit: "USD"
      };
    }
    return {
      isValid: false,
      invalidMessage: response.message || "Usage query failed"
    };
  }
})`;
}
