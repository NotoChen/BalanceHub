import type { InjectionKey, MaybeRefOrGetter } from "vue";
import type { Provider } from "../stores/provider-types";

export const providerBoardSections = [
  { key: "oauth", label: "OAuth 账号" },
  { key: "cpa", label: "CPA 网关" },
  { key: "liveness", label: "自动测活" },
  { key: "account", label: "账户认证" },
  { key: "apiKey", label: "API Key" },
] as const;

/** The rendered grids and drag boundaries must use the same sections. */
export function providerBoardSection(provider: Provider, liveness: boolean) {
  if (provider.actions.oauthAccount) return "oauth";
  if (provider.actions.cpaGateway && !liveness) return "cpa";
  if (liveness) return "liveness";
  return provider.auth.mode === "apiKey" ? "apiKey" : "account";
}

/// 卡片头部要知道自己所在分组是否已经写过协议名（NewAPI / Sub2API / 通用
/// API Key），从而不在副标题里重复一遍。看板按分组渲染时注入分组标题，
/// 独立展示卡片时没有这个值，副标题照旧写协议。
export const providerBoardSectionLabelKey: InjectionKey<MaybeRefOrGetter<string>> = Symbol(
  "balancehub.provider-board-section-label",
);
