import type {
  AuthMode,
  ProviderKind,
  ProviderProxyMode,
} from "../../stores/providers";
import type { SelectOption } from "../../utils/liveness-options";

export type { SelectOption } from "../../utils/liveness-options";
export {
  codexIntervalModeOptions,
  codexPromptModeOptions,
  livenessCliKindOptions,
  livenessHttpProtocolOptions,
  livenessMethodOptions,
} from "../../utils/liveness-options";

export const providerKindOptions: SelectOption<ProviderKind>[] = [
  { label: "NewAPI", value: "newApi" },
];

export const authModeOptions: SelectOption<AuthMode>[] = [
  { label: "会话 Cookie", value: "session" },
  { label: "访问令牌", value: "accessToken" },
  { label: "API 密钥", value: "apiKey" },
];

export const providerProxyModeOptions: SelectOption<ProviderProxyMode>[] = [
  { label: "跟随全局设置", value: "inherit" },
  { label: "跟随系统代理", value: "system" },
  { label: "不使用代理", value: "noProxy" },
  { label: "自定义代理", value: "custom" },
];
