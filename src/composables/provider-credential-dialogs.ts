import { h } from "vue";
import { Message, Modal } from "@arco-design/web-vue";

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
