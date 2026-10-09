import { h } from "vue";
import { Modal } from "@arco-design/web-vue";
import ProviderApiKeyEditorForm from "../components/provider-editor/ProviderApiKeyEditorForm.vue";
import type {
  ProviderApiKeyEditorContext,
  ProviderApiKeyPatch,
} from "../stores/provider-types";

export function openApiKeyEditor<T>(options: {
  editing: boolean;
  loadContext: () => Promise<ProviderApiKeyEditorContext>;
  submit: (
    patch: ProviderApiKeyPatch,
    credentialRevision: number,
  ) => Promise<T>;
}) {
  let active = true;
  let modal: ReturnType<typeof Modal.open> | undefined;
  let settle: (result: T | null) => void = () => {};
  const result = new Promise<T | null>((resolve) => {
    settle = resolve;
  });
  const finish = (value: T | null, close = true) => {
    if (!active) return;
    active = false;
    settle(value);
    if (close) modal?.close();
  };
  modal = Modal.open({
    title: options.editing ? "编辑站点 API Key" : "创建站点 API Key",
    width: 820,
    modalClass: ["surface-modal", "api-key-settings-modal"],
    footer: false,
    content: () =>
      h(ProviderApiKeyEditorForm, {
        ...options,
        isActive: () => active,
        onCancel: () => finish(null),
        onSaved: (value) => finish(value as T),
      }),
    onCancel: () => finish(null, false),
    onClose: () => finish(null, false),
  });
  return { result, close: () => finish(null) };
}
