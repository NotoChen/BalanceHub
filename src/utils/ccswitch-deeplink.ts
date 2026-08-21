import type { Provider } from "../stores/providers";
import { providerAgentBaseUrl } from "./cli-environment.ts";
import { providerDisplayLabel } from "./provider-display.ts";

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
  codex: "Codex CLI",
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
  const params = new URLSearchParams();
  params.set("resource", "provider");
  params.set("app", target);
  params.set("name", providerDisplayLabel(provider) || provider.identity.displayName.trim() || "BalanceHub");
  params.set("endpoint", endpointForTarget(provider, target));
  params.set("apiKey", provider.auth.apiKey.trim());

  return `ccswitch://v1/import?${params.toString()}`;
}

function endpointForTarget(provider: Provider, target: CcSwitchAppTarget) {
  if (target === "claude") {
    const raw = providerAgentBaseUrl(provider, "claudeCode");
    return normalizeUrl(raw);
  }
  if (OPENAI_TARGETS.has(target)) {
    const raw = providerAgentBaseUrl(provider, "codex");
    const normalized = normalizeUrl(raw);
    return normalized.endsWith("/v1") ? normalized : `${normalized}/v1`;
  }
  return normalizeUrl(provider.identity.baseUrl);
}

function normalizeUrl(value: string) {
  return value.trim().replace(/\/+$/, "");
}
