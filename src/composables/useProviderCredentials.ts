import { onUnmounted, provide, ref, shallowRef, watch, type InjectionKey } from "vue";
import { Message, Modal } from "@arco-design/web-vue";
import * as api from "../api/login-accounts";
import type { CredentialKind, ProviderCredentialDetails } from "../stores/provider-types";
import { withTimeout } from "../utils/promise-timeout";

export type ProviderCredentialsController = ReturnType<typeof useProviderCredentials>;
export const PROVIDER_CREDENTIALS_CONTEXT: InjectionKey<ProviderCredentialsController> = Symbol("provider-credentials");

/// 读取凭据、站点验证和清除都可能卡在网络或 IPC 上；没有超时时按钮会
/// 一直禁用，用户只能关闭窗口重开才能恢复。
const CREDENTIAL_TIMEOUT_MS = 30_000;
const credentialTimeoutMessage = (action: string) =>
  action === "validate"
    ? "验证站点凭据超时，请重试"
    : action === "clear"
      ? "清除本地凭据超时，请重新确认"
      : "读取站点凭据超时，请重试";

export function useProviderCredentials(relogin: (id: string) => void) {
  const visible = ref(false);
  const providerId = ref("");
  const details = shallowRef<ProviderCredentialDetails | null>(null);
  const pending = ref("");
  const error = ref("");
  const secretScope = ref(0);
  let revision = 0;
  function invalidate() { revision++; secretScope.value++; details.value = null; pending.value = ""; error.value = ""; }
  watch(visible, (value) => { if (!value) invalidate(); });
  onUnmounted(invalidate);
  async function run(action: string, work: (id: string) => Promise<ProviderCredentialDetails>) {
    if (pending.value) return;
    const request = ++revision;
    const id = providerId.value;
    pending.value = action; error.value = ""; secretScope.value++;
    try {
      const result = await withTimeout(
        work(id),
        CREDENTIAL_TIMEOUT_MS,
        credentialTimeoutMessage(action),
      );
      if (request !== revision || !visible.value || providerId.value !== id) return;
      details.value = result;
      if (action === "validate") {
        if (result.error) error.value = result.error;
        else Message.success("本次站点请求成功，验证范围见详情");
      }
    } catch (e) {
      if (request === revision && visible.value) error.value = e instanceof Error ? e.message : String(e);
    } finally { if (request === revision) pending.value = ""; }
  }
  function open(id: string) { invalidate(); providerId.value = id; visible.value = true; void run("load", api.getProviderCredentials); }
  function clear(kind: CredentialKind) {
    const expected = details.value;
    if (!expected) return;
    const entry = expected.entries.find((e) => e.kind === kind);
    if (!entry?.clearLabel) return;
    Modal.confirm({ title: entry.clearLabel,
      content: "只清除本机保存的这组凭据；JWT 与续期凭据会成组清除。站点账号、平台登录状态和其他站点凭据保留。此操作不代表服务端撤销授权。",
      okText: "清除本地凭据", cancelText: "取消", onOk: () => {
        if (providerId.value !== expected.providerId || !visible.value) return;
        void run("clear", async (id) => {
          await api.clearProviderCredential(id, kind, expected.credentialRevision);
          return api.getProviderCredentials(id);
        });
      } });
  }
  function login() { const id = providerId.value; visible.value = false; relogin(id); }
  const controller = { visible, providerId, details, pending, error, secretScope, open, clear, login,
    refresh: () => run("load", api.getProviderCredentials), validate: () => run("validate", api.validateProviderCredentials) };
  provide(PROVIDER_CREDENTIALS_CONTEXT, controller);
  return controller;
}
