import { onScopeDispose, ref, watch, type Ref } from "vue";
import type { Provider, ProviderInput, ProviderSaveConflict, ProviderSaveOptions, ProviderSaveResult } from "../stores/provider-types";
import { providerDuplicateSaveResolution, type ProviderDuplicateDecision, type ProviderSaveCompletion } from "./provider-editor-shared.ts";
import { withTimeout } from "../utils/promise-timeout.ts";

interface UseProviderSaveOptions {
  visible: Ref<boolean>;
  session: Ref<number>;
  input: () => ProviderInput;
  prepare: (isCurrent: () => boolean) => Promise<boolean | void>;
  canSave: () => boolean;
  save: (input: ProviderInput, options: ProviderSaveOptions) => Promise<ProviderSaveResult>;
  resolveConflict: (conflict: ProviderSaveConflict) => Promise<ProviderDuplicateDecision>;
  accept: (provider: Provider, completion: ProviderSaveCompletion) => void;
  completed: (provider: Provider) => void;
  timeoutMs?: number;
}

export function useProviderSave(options: UseProviderSaveOptions) {
  const saving = ref(false);
  const error = ref("");
  let revision = 0;
  let disposed = false;

  function invalidate() {
    revision += 1;
    saving.value = false;
    error.value = "";
  }
  watch([options.visible, options.session], invalidate, { flush: "sync" });
  onScopeDispose(() => { disposed = true; invalidate(); });

  async function saveDraft(isCurrent: () => boolean = () => true) {
    const session = options.session.value;
    const current = () => !disposed && options.visible.value && session === options.session.value && isCurrent();
    // A duplicate confirmation applies to exactly the submitted draft, even
    // if an unrelated background update arrives while the dialog is open.
    const input = JSON.parse(JSON.stringify(options.input())) as ProviderInput;
    let saveOptions: ProviderSaveOptions = {};
    let completion: ProviderSaveCompletion = "standard";
    while (current()) {
      const result = await withTimeout(
        options.save(input, saveOptions),
        options.timeoutMs ?? 15_000,
        "保存响应超时，请先核对中转站列表，再决定是否重试；当前填写内容已保留",
      );
      if (!current()) return;
      if (result.saved) {
        if (!result.provider) throw new Error("保存结果缺少中转站信息，请刷新列表核对");
        options.accept(result.provider, completion);
        return completion === "mergedApiKey" ? undefined : result.provider;
      }
      if (!result.conflict) return;
      const decision = await options.resolveConflict(result.conflict);
      if (!current()) return;
      const resolution = providerDuplicateSaveResolution(result.conflict, decision);
      if (!resolution) return;
      saveOptions = resolution.options;
      completion = resolution.completion;
    }
  }

  async function run() {
    if (disposed || saving.value || !options.visible.value || !options.canSave()) return;
    const request = ++revision;
    const session = options.session.value;
    const current = () => !disposed && request === revision && session === options.session.value && options.visible.value;
    saving.value = true;
    error.value = "";
    try {
      const prepared = await options.prepare(current);
      if (!current() || prepared === false) return;
      const provider = await saveDraft(current);
      if (provider && current()) {
        options.visible.value = false;
        options.completed(provider);
      }
    } catch (failure) {
      if (current()) error.value = failure instanceof Error ? failure.message : String(failure);
    } finally {
      if (request === revision) saving.value = false;
    }
  }

  return { saving, error, run, saveDraft };
}
