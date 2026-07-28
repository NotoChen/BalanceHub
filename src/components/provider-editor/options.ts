import type { ProviderProxyMode } from "../../stores/providers";
import type { SelectOption } from "../../utils/liveness-options";

export type { SelectOption } from "../../utils/liveness-options";
export {
  codexIntervalModeOptions,
  codexPromptModeOptions,
} from "../../utils/liveness-options";

export const providerProxyModeOptions: SelectOption<ProviderProxyMode>[] = [
  { label: "跟随全局设置", value: "inherit" },
  { label: "跟随系统代理", value: "system" },
  { label: "不使用代理", value: "noProxy" },
  { label: "自定义代理", value: "custom" },
];
