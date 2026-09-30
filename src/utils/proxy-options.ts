import type { ProviderProxyMode, ProxyMode } from "../stores/providers";
import type { SelectOption } from "./liveness-options";

export const proxyModeOptions: SelectOption<ProxyMode>[] = [
  { label: "跟随系统代理", value: "system" },
  { label: "不使用代理", value: "noProxy" },
  { label: "自定义代理", value: "custom" },
];

export const providerProxyModeOptions: SelectOption<ProviderProxyMode>[] = [
  { label: "跟随全局设置", value: "inherit" },
  ...proxyModeOptions,
];
