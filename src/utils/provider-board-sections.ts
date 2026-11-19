import type { InjectionKey, MaybeRefOrGetter } from "vue";
import type { Provider } from "../stores/provider-types";

/// Board sections describe object types, not capabilities. Auth mode, protocol
/// and automatic liveness are badges inside a card, so enabling liveness never
/// moves a card to another grid and drag boundaries stay stable.
export const providerBoardSections = [
  { key: "station", label: "中转站" },
  { key: "oauth", label: "OAuth 账号" },
  { key: "cpa", label: "CPA 网关" },
] as const;

export type ProviderBoardSectionKey = (typeof providerBoardSections)[number]["key"];

/** The rendered grids and drag boundaries must use the same sections. */
export function providerBoardSection(provider: Provider): ProviderBoardSectionKey {
  if (provider.actions.oauthAccount) return "oauth";
  if (provider.actions.cpaGateway) return "cpa";
  return "station";
}

/// 卡片头部要知道自己所在分组是否已经写过协议名（NewAPI / Sub2API / 通用
/// API Key），从而不在副标题里重复一遍。看板按分组渲染时注入分组标题，
/// 独立展示卡片时没有这个值，副标题照旧写协议。
export const providerBoardSectionLabelKey: InjectionKey<MaybeRefOrGetter<string>> = Symbol(
  "balancehub.provider-board-section-label",
);
