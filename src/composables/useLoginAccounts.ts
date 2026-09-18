import { computed, onUnmounted, provide, ref, shallowRef, watch, type InjectionKey } from "vue";
import { Message } from "@arco-design/web-vue";
import * as api from "../api/login-accounts";
import { cancelProviderBrowserLogin } from "../api/provider-browser-login";
import type { LoginAccountSummary, LoginCookieSummary, LoginPlatform } from "../stores/provider-types";
import type { BrowserRuntimeController } from "./useBrowserRuntime";
import { withTimeout } from "../utils/promise-timeout";
import { createLoginAccountSelection, type LoginAccountSelectionView, type LoginAccountTarget } from "../utils/login-account-selection";

export type LoginAccountsController = ReturnType<typeof useLoginAccounts>;
export const LOGIN_ACCOUNTS_CONTEXT: InjectionKey<LoginAccountsController> = Symbol("login-accounts");
export const LOGIN_PLATFORM_OPTIONS = [
  { label: "Linux DO", value: "linuxDo" },
  { label: "GitHub", value: "github" },
  { label: "站点账号 / 其他平台", value: "other" },
  { label: "暂不指定", value: "unknown" },
];

export function useLoginAccounts(runtime: BrowserRuntimeController) {
  const visible = ref(false);
  const selectionView = shallowRef<LoginAccountSelectionView>({ target: null, creating: false, error: "" });
  const selection = createLoginAccountSelection({
    create: api.createLoginAccount,
    changed: (next) => {
      const wasChoosing = Boolean(selectionView.value.target);
      selectionView.value = next;
      if (wasChoosing && !next.target) visible.value = false;
    },
  });
  const choosing = computed(() => Boolean(selectionView.value.target));
  const accounts = shallowRef<LoginAccountSummary[]>([]);
  const selectedId = ref("");
  const cookies = shallowRef<LoginCookieSummary[]>([]);
  const pending = ref("");
  const error = ref("");
  let revision = 0;
  let listRequest = 0;
  const selected = computed(() => accounts.value.find((a) => a.id === selectedId.value) ?? null);

  function cancelSelection() {
    selection.cancel();
  }
  function reset() { revision++; pending.value = ""; error.value = ""; cookies.value = []; }
  watch(visible, (value) => { if (!value) { reset(); cancelSelection(); } });
  watch(selectedId, reset);
  onUnmounted(() => { reset(); cancelSelection(); });

  async function refresh() {
    const request = ++listRequest;
    const result = await api.listLoginAccounts();
    if (request === listRequest) {
      accounts.value = result;
      if (selectedId.value && !result.some((a) => a.id === selectedId.value)) selectedId.value = "";
    }
  }
  async function operate(action: string, work: () => Promise<void>) {
    if (pending.value) return;
    const request = ++revision;
    pending.value = action; error.value = "";
    try { await work(); }
    catch (e) { if (request === revision && visible.value) error.value = String(e instanceof Error ? e.message : e); }
    finally { if (request === revision) { pending.value = ""; revision++; } }
  }
  function open(id?: string) {
    cancelSelection(); reset(); visible.value = true;
    if (id) selectedId.value = id;
    void operate("list", refresh);
  }
  function choose(target: LoginAccountTarget) {
    cancelSelection(); reset();
    selectedId.value = target.previousAccountId ?? "";
    const choice = selection.open(target);
    visible.value = true;
    void operate("list", refresh);
    return choice;
  }
  function confirmChoice() {
    if (!selected.value?.canLogin || pending.value) return;
    selection.confirm(selected.value.id);
  }
  function save(name: string, platform: LoginPlatform, create: boolean) {
    const id = selectedId.value;
    return operate("save", async () => {
      const request = revision;
      const created = create ? await api.createLoginAccount(name, platform) : null;
      if (!create) await api.updateLoginAccount(id, name, platform);
      await refresh();
      if (request !== revision || !visible.value) return;
      if (created) selectedId.value = created.id;
      Message.success(create ? "登录账号已保存，可用此账号登录站点" : "账号备注已保存");
    });
  }
  function remove(removeEntry: boolean) {
    const id = selectedId.value;
    return operate("remove", async () => {
      const request = revision;
      await api.removeLoginAccount(id, removeEntry); await refresh();
      if (request !== revision || !visible.value || selectedId.value !== id) return;
      cookies.value = []; Message.success(removeEntry ? "已删除本地账号；站点凭据仍保留" : "已清除该账号的本地登录状态");
    });
  }
  function inspectCookies() {
    const id = selectedId.value;
    return operate("cookies", async () => {
      const request = revision;
      const result = await api.listLoginCookies(id);
      if (request === revision && selectedId.value === id && visible.value) cookies.value = result;
    });
  }
  function launch(authorizations = false) {
    const id = selectedId.value;
    return operate("open", async () => {
      const request = revision;
      await runtime.refresh(true);
      if (request !== revision || !visible.value) return;
      if (!runtime.state.value?.ready) { runtime.open(); return; }
      let accepted = false;
      const starting = api.openLoginAccount(id, authorizations);
      void starting.then((task) => {
        if (!accepted && (request !== revision || !visible.value)) return cancelProviderBrowserLogin(task.runId);
      }).catch(() => {});
      await withTimeout(starting, 10_000, "打开账号窗口超时，请重试");
      if (request !== revision || !visible.value) return;
      accepted = true; visible.value = false;
      Message.info("账号窗口正在打开，完成后关闭小窗即可保存；后台任务中可以取消");
    });
  }
  const controller = { visible, choosing, selectionView, accounts, selectedId, selected, cookies, pending, error, open, choose, cancelSelection, confirmChoice,
    createAndChoose: selection.create, reload: () => operate("list", refresh), save, remove, inspectCookies, launch };
  provide(LOGIN_ACCOUNTS_CONTEXT, controller);
  return controller;
}
