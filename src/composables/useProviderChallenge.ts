import { ref } from "vue";
import { Message } from "@arco-design/web-vue";
import type { Provider } from "../stores/providers";

interface UseProviderChallengeOptions {
  passChallenge: (providerId: string) => Promise<string>;
}

/// 手动为盾后站点完成一次 Cloudflare 人机验证。
///
/// 后台刷新一律静默，绝不弹窗；只有用户点这里才会打开验证窗口。通过之后
/// 凭证会被缓存复用，短期内不需要重复验证，所以这里顺手刷新一次该站点。
export function useProviderChallenge(options: UseProviderChallengeOptions) {
  const challengingProviderId = ref<string | null>(null);

  async function passChallenge(provider: Provider) {
    const id = provider.identity.id;
    if (challengingProviderId.value) {
      return;
    }
    challengingProviderId.value = id;
    try {
      const message = await options.passChallenge(id);
      Message.success(message || "站点验证已通过");
    } catch (error) {
      Message.error(error instanceof Error ? error.message : String(error));
    } finally {
      challengingProviderId.value = null;
    }
  }

  return { challengingProviderId, passProviderChallenge: passChallenge };
}
