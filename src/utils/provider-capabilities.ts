import type { ProviderOptionalCapability } from "../stores/provider-types";

/// 站点可选能力的展示名只保留一份：能力探测弹窗、详情面板和后续入口都读这里，
/// 同一个接口在任何界面都叫同一个名字。
export const providerOptionalCapabilityLabels: Record<
  ProviderOptionalCapability,
  string
> = {
  pricing: "站点定价",
  performance: "模型表现",
  usage: "用量趋势",
  requestLogs: "请求日志",
  announcements: "站点公告",
  subscription: "订阅额度",
  groups: "站点分组",
  models: "模型列表",
};

export function providerOptionalCapabilityLabel(
  capability: ProviderOptionalCapability,
) {
  return providerOptionalCapabilityLabels[capability] ?? capability;
}

/// 能力台账的写入来源名只维护一份；弹窗和详情面读同一条结论时不能出现
/// 两种说法，也不能把未知来源误写成「本地观测」以外的含义。
export function providerCapabilitySourceLabel(source?: string | null) {
  switch (source?.trim()) {
    case "probe":
      return "能力探测";
    case "refresh":
      return "普通刷新";
    case "manual":
      return "手动同步";
    default:
      return source?.trim() || "本地观测";
  }
}
