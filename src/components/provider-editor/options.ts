import type { AuthMode, ProviderProxyMode } from "../../stores/providers";
import type { SelectOption } from "../../utils/liveness-options";

export type { SelectOption } from "../../utils/liveness-options";
export {
  codexIntervalModeOptions,
  codexPromptModeOptions,
  livenessCliKindOptions,
} from "../../utils/liveness-options";

export const authModeOptions: SelectOption<AuthMode>[] = [
  { label: "账号密码", value: "password" },
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
