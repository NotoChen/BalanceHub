import type { AgentCliKind, Provider } from "../stores/providers";
import { providerAgentBaseUrl } from "./cli-environment.ts";
import { providerDisplayLabel } from "./provider-display.ts";

// CC Switch v4.0.8: deeplink/parser.rs and deeplink/provider.rs.
// Gemini and Claude consume their native API root; the other targets use /v1.
const targetDefinitions = {
  codex: { label: "Codex CLI", endpointKind: "codex", versioned: true },
  claude: { label: "Claude Code", endpointKind: "claudeCode", versioned: false },
  gemini: { label: "Gemini CLI", endpointKind: "gemini", versioned: false },
  grokbuild: { label: "Grok Build", endpointKind: "grok", versioned: true },
  opencode: { label: "OpenCode", endpointKind: "codex", versioned: true },
  openclaw: { label: "OpenClaw", endpointKind: "codex", versioned: true },
  hermes: { label: "Hermes", endpointKind: "codex", versioned: true },
} as const satisfies Record<string, { label: string; endpointKind: AgentCliKind; versioned: boolean }>;

export type CcSwitchAppTarget = keyof typeof targetDefinitions;
export const ccSwitchTargets = Object.keys(targetDefinitions) as CcSwitchAppTarget[];
export const ccSwitchTargetLabels = Object.fromEntries(
  ccSwitchTargets.map((target) => [target, targetDefinitions[target].label]),
) as Record<CcSwitchAppTarget, string>;

export function canBuildCcSwitchDeeplink(provider: Provider) {
  return Boolean(provider.identity.baseUrl.trim() && provider.auth.apiKey.trim());
}

export function buildCcSwitchProviderDeeplink(
  provider: Provider,
  target: CcSwitchAppTarget,
) {
  const params = new URLSearchParams();
  params.set("resource", "provider");
  params.set("app", target);
  params.set("name", providerDisplayLabel(provider) || provider.identity.displayName.trim() || "BalanceHub");
  params.set("endpoint", endpointForTarget(provider, target));
  params.set("apiKey", provider.auth.apiKey.trim());
  const model = provider.cli.preferredModel.trim();
  if (model) params.set("model", model);

  return `ccswitch://v1/import?${params.toString()}`;
}

function endpointForTarget(provider: Provider, target: CcSwitchAppTarget) {
  const definition = targetDefinitions[target];
  const normalized = normalizeUrl(providerAgentBaseUrl(provider, definition.endpointKind));
  return definition.versioned && !normalized.endsWith("/v1") ? `${normalized}/v1` : normalized;
}

function normalizeUrl(value: string) {
  return value.trim().replace(/\/+$/, "");
}
