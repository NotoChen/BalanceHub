import { h } from "vue";
import { Button, Message, Modal } from "@arco-design/web-vue";
import { IconLink, IconPlus } from "@arco-design/web-vue/es/icon";
import type { ProviderDuplicateDecision } from "./provider-editor-shared";

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
) {
  return new Promise<boolean>((resolve) => {
    let settled = false;
    Modal.confirm({
      title,
      content,
      okText,
      cancelText: "取消",
      okButtonProps: status === "normal" ? undefined : { status },
      onOk: () => {
        settled = true;
        resolve(true);
      },
      onCancel: () => {
        settled = true;
        resolve(false);
      },
      onClose: () => {
        if (!settled) {
          resolve(false);
        }
      },
    });
  });
}

export function promptApiKeyName() {
  return new Promise<string | null>((resolve) => {
    let value = "";
    let settled = false;
    Modal.confirm({
      title: "创建 API 密钥",
      okText: "创建",
      cancelText: "取消",
      content: () =>
        h("div", { class: "api-key-create-form" }, [
          h("label", { class: "api-key-create-label", for: "provider-editor-api-key-name" }, "密钥名称"),
          h("input", {
            id: "provider-editor-api-key-name",
            class: "arco-input arco-input-size-medium",
            placeholder: "例如：个人电脑、Claude Code、备用密钥",
            autofocus: true,
            onInput: (event: Event) => {
              value = (event.target as HTMLInputElement).value;
            },
          }),
        ]),
      onBeforeOk: () => {
        if (!value.trim()) {
          Message.warning("请填写 API 密钥名称");
          return false;
        }
        settled = true;
        resolve(value.trim());
        return true;
      },
      onCancel: () => {
        settled = true;
        resolve(null);
      },
      onClose: () => {
        if (!settled) {
          resolve(null);
        }
      },
    });
  });
}
