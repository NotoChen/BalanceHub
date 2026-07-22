import type { Provider, ProviderInput } from "../stores/providers";
import {
  DEFAULT_LIVENESS_INTERVAL,
  DEFAULT_LIVENESS_RANDOM_MIN_INTERVAL,
} from "./liveness-defaults";

/// 新建中转站时的空白草稿。集中在此，避免多处手写同一份字段列表导致漂移。
export function emptyDraft(): ProviderInput {
  return {
    id: undefined,
    identity: {
      name: "",
      baseUrl: "",
      backupUrls: [],
    },
    auth: {
      mode: "password",
      apiKey: "",
      apiKeyTokenId: "",
      apiKeyOptions: [],
      accessToken: "",
      sessionCookie: "",
      apiUser: "",
      loginUsername: "",
      loginPassword: "",
    },
    cli: {
      preferredModel: "",
    },
    automation: {
      refreshInterval: 0,
      checkInTime: "",
    },
    proxy: {
      mode: "inherit",
      url: "",
    },
    notification: {
      mode: "inherit",
      channelIds: [],
    },
    liveness: {
      useGlobal: true,
      enabled: false,
      openaiBaseUrl: "",
      anthropicBaseUrl: "",
      cliKind: null,
      intervalMode: "fixed",
      interval: DEFAULT_LIVENESS_INTERVAL,
      randomMinInterval: DEFAULT_LIVENESS_RANDOM_MIN_INTERVAL,
      randomMaxInterval: DEFAULT_LIVENESS_INTERVAL,
      timeout: 75,
      model: "",
      promptMode: "random",
      fixedPrompt: "",
    },
    runtime: {
      enabled: true,
    },
  };
}

/// 把已保存的 Provider 映射为可提交的 ProviderInput，可用 overrides 覆盖个别字段。
/// 之前 store.toggleProvider / useManagedApiKey / openEditProvider 各自手抄一份字段列表，
/// 新增字段时极易漏改某处造成静默丢字段，这里统一为单一来源。
export function providerToInput(
  provider: Provider,
  overrides: Partial<ProviderInput> = {},
): ProviderInput {
  return {
    id: provider.identity.id,
    identity: {
      name: provider.identity.name,
      baseUrl: provider.identity.baseUrl,
      backupUrls: [...(provider.identity.backupUrls || [])],
    },
    auth: {
      ...provider.auth,
      apiKeyOptions: [...(provider.auth.apiKeyOptions || [])],
    },
    cli: {
      preferredModel: provider.cli?.preferredModel || "",
    },
    automation: {
      refreshInterval: provider.automation.refreshInterval,
      checkInTime: provider.automation.checkInTime,
    },
    proxy: { ...provider.proxy },
    notification: {
      mode: provider.notification.mode,
      channelIds: [...(provider.notification.channelIds || [])],
    },
    liveness: {
      useGlobal: provider.liveness.useGlobal,
      enabled: provider.liveness.enabled,
      openaiBaseUrl: provider.liveness.openaiBaseUrl,
      anthropicBaseUrl: provider.liveness.anthropicBaseUrl,
      cliKind: provider.liveness.cliKind ?? null,
      intervalMode: provider.liveness.intervalMode,
      interval: provider.liveness.interval,
      randomMinInterval: provider.liveness.randomMinInterval,
      randomMaxInterval: provider.liveness.randomMaxInterval,
      timeout: provider.liveness.timeout,
      model: provider.liveness.model,
      promptMode: provider.liveness.promptMode,
      fixedPrompt: provider.liveness.fixedPrompt,
    },
    runtime: {
      enabled: provider.runtime.enabled,
    },
    ...overrides,
  };
}
