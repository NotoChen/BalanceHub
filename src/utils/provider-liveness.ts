import type { AppSettings, Provider } from "../stores/providers";

export function providerLivenessEnabled(provider: Provider, settings: AppSettings) {
  if (!provider.runtime.enabled || !provider.auth.apiKey.trim()) {
    return false;
  }
  return provider.liveness.useGlobal
    ? settings.livenessEnabled
    : provider.liveness.enabled;
}
