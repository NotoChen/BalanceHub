import type { LoginPlatform } from "../stores/provider-types.ts";
import { withTimeout } from "./promise-timeout.ts";

export interface LoginAccountTarget {
  name: string;
  baseUrl: string;
  previousAccountId: string | null;
}

export interface LoginAccountSelectionView {
  target: LoginAccountTarget | null;
  creating: boolean;
  error: string;
}

interface SelectionOptions {
  create: (name: string, platform: LoginPlatform) => Promise<{ id: string }>;
  changed: (view: LoginAccountSelectionView) => void;
  timeoutMs?: number;
}

/** Owns the pending site choice, including account creation across modal closes. */
export function createLoginAccountSelection(options: SelectionOptions) {
  let revision = 0;
  let view: LoginAccountSelectionView = { target: null, creating: false, error: "" };
  let resolveChoice: ((id: string | null) => void) | null = null;
  function update(next: LoginAccountSelectionView) { view = next; options.changed({ ...view }); }
  function finish(id: string | null) {
    revision++;
    const resolve = resolveChoice;
    resolveChoice = null;
    update({ target: null, creating: false, error: "" });
    resolve?.(id);
  }
  function open(target: LoginAccountTarget) {
    finish(null);
    update({ target: { ...target }, creating: false, error: "" });
    return new Promise<string | null>((resolve) => { resolveChoice = resolve; });
  }
  function confirm(id: string) {
    if (view.target && !view.creating) finish(id);
  }
  async function create(name: string, platform: LoginPlatform) {
    if (!view.target || view.creating) return;
    const request = ++revision;
    update({ ...view, creating: true, error: "" });
    try {
      const account = await withTimeout(options.create(name, platform), options.timeoutMs ?? 10_000,
        "创建登录账号超时，可重新打开账号列表确认结果");
      if (request === revision && view.target) finish(account.id);
    } catch (error) {
      if (request === revision && view.target) {
        update({ ...view, creating: false, error: error instanceof Error ? error.message : String(error) });
      }
    } finally {
      if (request === revision && view.creating) update({ ...view, creating: false });
    }
  }
  return { open, confirm, cancel: () => finish(null), create };
}
