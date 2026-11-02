import { invoke } from "./transport.ts";
import { withTimeout } from "../utils/promise-timeout";

export function openProviderSite(id: string) {
  return withTimeout(invoke<void>("open_provider_site", { id }), 8_000, "打开中转站超时");
}
