import { Message } from "@arco-design/web-vue";
import type { Provider } from "../stores/providers";
import { normalizeInviteLink } from "../utils/provider-display";
import { copyText } from "./useClipboard";

interface UseProviderCopyActionsOptions {
  getInviteLink: (id: string) => Promise<string>;
  reload: () => Promise<unknown>;
  setBusyProviderId: (id: string | null) => void;
}

export function useProviderCopyActions(options: UseProviderCopyActionsOptions) {
  async function copyProviderUrl(provider: Provider) {
    const value = provider.identity.baseUrl.trim();
    if (!value) {
      Message.warning("中转站 URL 为空");
      return;
    }

    try {
      await copyText(value);
      Message.success("已复制中转站 URL");
    } catch (error) {
      Message.error(error instanceof Error ? error.message : String(error));
    }
  }

  async function copyProviderSecret(
    provider: Provider,
    field: "apiKey" | "accessToken" | "sessionCookie",
  ) {
    const labels = {
      apiKey: "API 密钥",
      accessToken: "访问令牌",
      sessionCookie: "Cookie",
    } as const;
    const values = {
      apiKey: provider.auth.apiKey.trim(),
      accessToken: provider.auth.accessToken.trim(),
      sessionCookie: provider.auth.sessionCookie.trim(),
    } as const;
    const label = labels[field];
    const value = values[field];
    if (!value) {
      Message.warning(`${label}为空`);
      return;
    }

    try {
      await copyText(value);
      Message.success(`已复制${label}`);
    } catch (error) {
      Message.error(error instanceof Error ? error.message : String(error));
    }
  }

  async function copyInviteLink(provider: Provider) {
    if (!provider.runtime.enabled) {
      return;
    }

    options.setBusyProviderId(provider.identity.id);
    try {
      const link = normalizeInviteLink(provider.capabilities.inviteLink || (await options.getInviteLink(provider.identity.id)));
      await copyText(link);
      await options.reload();
      Message.success("已复制邀请链接");
    } catch (error) {
      Message.error(error instanceof Error ? error.message : String(error));
    } finally {
      options.setBusyProviderId(null);
    }
  }

  return {
    copyProviderUrl,
    copyProviderSecret,
    copyInviteLink,
  };
}
