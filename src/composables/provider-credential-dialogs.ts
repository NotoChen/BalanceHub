import { h, ref } from "vue";
import { Button, Modal } from "@arco-design/web-vue";
import { IconLink, IconPlus } from "@arco-design/web-vue/es/icon";
import type { ProviderDuplicateDecision } from "./provider-editor-shared";
import type { ProviderApiKeyOption } from "../stores/providers";
import { maskApiKey, providerApiKeyDisplayName } from "../utils/provider-display";
import RadioChoiceGroup from "../components/RadioChoiceGroup.vue";

export function chooseProviderApiKey(keys: ProviderApiKeyOption[], signal: AbortSignal) {
  return new Promise<ProviderApiKeyOption | null>((resolve) => {
    if (signal.aborted) { resolve(null); return; }
    const selected = ref("");
    let settled = false;
    let modal: ReturnType<typeof Modal.open> | undefined;
    const finish = (key: ProviderApiKeyOption | null, close = true) => {
      if (settled) return;
      settled = true;
      signal.removeEventListener("abort", cancel);
      resolve(key);
      if (close) modal?.close();
    };
    const cancel = () => finish(null);
    signal.addEventListener("abort", cancel, { once: true });
    const choices = keys.map((key, index) => ({
      value: String(index),
      label: providerApiKeyDisplayName(key),
      description: [key.group || key.groupId, maskApiKey(key.key)].filter(Boolean).join(" · "),
    }));
    modal = Modal.open({
      title: "选择当前调用 API Key",
      width: 560,
      modalClass: ["surface-modal", "provider-key-selection-modal"],
      footer: false,
      content: () => h("div", { class: "provider-key-selection" }, [
        h("p", "已读取多把 Key，请选择此中转站默认使用的一把。"),
        h(RadioChoiceGroup, {
          modelValue: selected.value,
          options: choices,
          label: "当前调用 API Key",
          class: "provider-key-selection-options",
          optionClass: "provider-key-selection-option",
          "onUpdate:modelValue": (value: string) => { selected.value = value; },
        }, { default: ({ option }: { option: typeof choices[number] }) => [
          h("strong", option.label), h("small", option.description),
        ] }),
        h("div", { class: "provider-duplicate-actions" }, [
          h(Button, { onClick: cancel }, { default: () => "取消" }),
          h(Button, { type: "primary", disabled: selected.value === "", onClick: () => finish(keys[Number(selected.value)] ?? null) }, { default: () => "使用此 Key 并继续" }),
        ]),
      ]),
      onCancel: () => finish(null, false),
      onClose: () => finish(null, false),
    });
  });
}

export function chooseSameSiteApiKeyAction(existingName: string) {
  return new Promise<ProviderDuplicateDecision>((resolve) => {
    let settled = false;
    let modal: ReturnType<typeof Modal.open> | undefined;

    const settle = (decision: ProviderDuplicateDecision, close = true) => {
      if (settled) return;
      settled = true;
      resolve(decision);
      if (close) modal?.close();
    };

    modal = Modal.open({
      title: "保存当前 API Key",
      width: 540,
      modalClass: ["surface-modal", "provider-duplicate-modal"],
      footer: false,
      content: () =>
        h("div", { class: "provider-duplicate-dialog" }, [
          h(
            "p",
            { class: "provider-duplicate-message" },
            `同一地址下已存在“${existingName}”。请选择把当前 API Key 保存为独立卡片，或加入已有卡片的认证凭据。`,
          ),
          h("div", { class: "provider-duplicate-actions" }, [
            h(
              Button,
              { onClick: () => settle("cancel") },
              { default: () => "取消" },
            ),
            h(
              Button,
              { type: "secondary", onClick: () => settle("merge") },
              {
                icon: () => h(IconLink),
                default: () => "加入已有卡片",
              },
            ),
            h(
              Button,
              { type: "primary", onClick: () => settle("createSeparate") },
              {
                icon: () => h(IconPlus),
                default: () => "创建独立卡片",
              },
            ),
          ]),
        ]),
      onCancel: () => settle("cancel", false),
      onClose: () => settle("cancel", false),
    });
  });
}

export function confirmAction(
  title: string,
  content: string,
  okText: string,
  status: "normal" | "warning" | "danger" = "normal",
  signal?: AbortSignal,
) {
  return new Promise<boolean>((resolve) => {
    if (signal?.aborted) { resolve(false); return; }
    let settled = false;
    let modal: ReturnType<typeof Modal.confirm> | undefined;
    const finish = (confirmed: boolean) => {
      if (settled) return;
      settled = true;
      signal?.removeEventListener("abort", cancel);
      resolve(confirmed);
    };
    const cancel = () => { finish(false); modal?.close(); };
    signal?.addEventListener("abort", cancel, { once: true });
    modal = Modal.confirm({
      title,
      content,
      okText,
      cancelText: "取消",
      okButtonProps: status === "normal" ? undefined : { status },
      onOk: () => finish(true),
      onCancel: () => finish(false),
      onClose: () => finish(false),
    });
  });
}
