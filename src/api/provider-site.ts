import { invoke, isWebHost } from "./transport.ts";
import { withTimeout } from "../utils/promise-timeout";

export function openProviderSite(id: string) {
  // 浏览器版不能读取本机配置里的地址，由服务端解析后再打开新标签页。
  if (isWebHost) {
    return withTimeout(
      invoke<string>("get_provider_site_url", { id }).then((url) => {
        window.open(url, "_blank", "noopener,noreferrer");
      }),
      8_000,
      "打开中转站超时",
    );
  }
  return withTimeout(invoke<void>("open_provider_site", { id }), 8_000, "打开中转站超时");
}
