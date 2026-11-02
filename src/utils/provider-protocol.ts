import type {
  AuthMode,
  ProviderProtocol,
  ProviderProtocolDescriptor,
} from "../stores/providers";

export function providerProtocolDescriptor(
  descriptors: ProviderProtocolDescriptor[],
  kind: ProviderProtocol,
) {
  return descriptors.find((descriptor) => descriptor.kind === kind);
}

/// 四类卡片、底栏、确认弹窗和编辑窗口对同一对象的称呼。协议决定它是中转站、
/// 授权账号还是网关，同一句话在这些位置只写一次。
export function providerScopeNoun(protocol: ProviderProtocol): string {
  if (protocol === "cliProxyApi") return "网关";
  if (protocol === "oauth") return "账号";
  return "中转站";
}

/// 编辑窗口标题。称呼跟着此刻选中的协议走：从「CPA 网关」入口打开时是
/// 「添加网关」，在类型选择里换成 NewAPI 时就地改口成「添加中转站」。
export function providerEditorTitle(editing: boolean, protocol: ProviderProtocol) {
  return `${editing ? "编辑" : "添加"}${providerScopeNoun(protocol)}`;
}

export function providerProtocolLabel(
  descriptors: ProviderProtocolDescriptor[],
  kind: ProviderProtocol,
) {
  return providerProtocolDescriptor(descriptors, kind)?.label ?? kind;
}

export function providerAuthModeLabel(
  descriptors: ProviderProtocolDescriptor[],
  mode: AuthMode,
) {
  for (const descriptor of descriptors) {
    const authMode = descriptor.authModes.find((candidate) => candidate.mode === mode);
    if (authMode) return authMode.label;
  }
  return mode;
}

export function providerAuthModeDescriptor(
  descriptors: ProviderProtocolDescriptor[],
  protocol: ProviderProtocol,
  mode: AuthMode,
) {
  return providerProtocolDescriptor(descriptors, protocol)?.authModes.find(
    (candidate) => candidate.mode === mode,
  );
}
