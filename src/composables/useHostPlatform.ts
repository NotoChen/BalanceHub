import { readonly, shallowRef } from "vue";
import { hostPlatform } from "../api/app";
import { withTimeout } from "../utils/promise-timeout";

const platform = shallowRef<string | null>(null);
let pending: Promise<string | null> | null = null;

function ensure(): Promise<string | null> {
  if (platform.value !== null) return Promise.resolve(platform.value);
  if (pending) return pending;
  pending = withTimeout(
    Promise.resolve().then(hostPlatform),
    5_000,
    "读取系统类型超时",
  )
    .then((value) => {
      platform.value = value;
      return value;
    })
    .catch(() => null)
    .finally(() => {
      pending = null;
    });
  return pending;
}

export function useHostPlatform() {
  return { platform: readonly(platform), ensure };
}
