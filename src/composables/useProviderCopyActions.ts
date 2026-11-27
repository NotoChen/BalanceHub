import { Message } from "@arco-design/web-vue";
import type { Provider } from "../stores/providers";
import { copyText } from "./useClipboard";

interface UseProviderCopyActionsOptions {
  getInviteLink: (id: string) => Promise<string>;
  /// 只补这张卡片的视图；邀请链接落在中转站视图上，不需要重读整份配置。
  reload: (providerId: string) => Promise<unknown>;
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
      const link = provider.capabilities.inviteLink || (await options.getInviteLink(provider.identity.id));
      await copyText(link);
      // 链接已经写进剪贴板；列表刷新只是顺带更新展示，失败不能把这次复制
      // 说成失败，否则用户会以为没复制而重复操作。
      Message.success("已复制邀请链接");
      try {
        await options.reload(provider.identity.id);
      } catch (error) {
        Message.warning(
          `邀请链接已复制，但列表刷新失败：${error instanceof Error ? error.message : String(error)}`,
        );
      }
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
