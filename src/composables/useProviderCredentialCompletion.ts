import { computed, h, ref, watch, type Ref } from "vue";
import { Message, Modal } from "@arco-design/web-vue";
import type { Provider, ProviderApiKeyOption, ProviderInput, ProviderSiteProbeResult } from "../stores/providers";
import { fieldLabel, normalizeProviderBaseUrl } from "./provider-editor-shared";

export type CredentialCompletionState =
  | "idle"
  | "probingSite"
  | "resolvingCredentials"
  | "needAccessTokenConfirm"
  | "generatingAccessToken"
  | "needApiKeySelection"
  | "needApiKeyName"
  | "creatingApiKey"
  | "saving"
  | "done"
  | "failed";

export interface CredentialCompletionStep {
  key: string;
  name: string;
  status: "pending" | "running" | "done" | "error" | "skipped";
  message: string;
}

interface UseProviderCredentialCompletionOptions {
  draftProvider: ProviderInput;
  editingProviderId: Ref<string | null>;
  probingSite: Ref<boolean>;
  siteProbeResult: Ref<ProviderSiteProbeResult | null>;
  completingCredentials: Ref<boolean>;
  credentialCompletionMessage: Ref<string>;
  credentialCompletionSteps: Ref<{ name: string; ok: boolean; message: string }[]>;
  siteNameSourceBaseUrl: Ref<string>;
  probeProviderSite: (input: ProviderInput) => Promise<ProviderSiteProbeResult>;
  completeProviderCredentials: (input: ProviderInput) => Promise<{
    input: ProviderInput;
    changedFields: string[];
    steps: { name: string; ok: boolean; message: string }[];
    apiKeyOptions: ProviderApiKeyOption[];
  }>;
  createApiKeyForInput: (input: ProviderInput, name: string) => Promise<ProviderApiKeyOption>;
  generateAccessTokenForInput: (input: ProviderInput) => Promise<string>;
  setApiKeyOptions: (options: ProviderApiKeyOption[]) => void;
  saveDraftAndFindProvider: () => Promise<Provider | undefined>;
  refreshAfterSave: (provider: Provider | undefined) => void;
}

export function useProviderCredentialCompletion(options: UseProviderCredentialCompletionOptions) {
  interface CompletionRunOptions {
    notify?: boolean;
    save?: boolean;
  }

  const credentialAssistantState = ref<CredentialCompletionState>("idle");
  const credentialAssistantSteps = ref<CredentialCompletionStep[]>([]);
  const credentialAssistantMessage = ref("");
  const credentialAssistantChangedFields = ref<string[]>([]);
  const credentialAssistantSaved = ref(false);

  const credentialAssistantBusy = computed(() =>
    [
      "probingSite",
      "resolvingCredentials",
      "generatingAccessToken",
      "creatingApiKey",
      "saving",
    ].includes(credentialAssistantState.value),
  );

  const canRunCredentialAssistant = computed(() => {
    if (credentialAssistantBusy.value || options.draftProvider.auth.mode === "apiKey") {
      return false;
    }
    if (!options.draftProvider.identity.baseUrl.trim()) {
      return false;
    }
    if (options.draftProvider.auth.mode === "session") {
      return Boolean(options.draftProvider.auth.sessionCookie.trim());
    }
    if (options.draftProvider.auth.mode === "password") {
      return Boolean(
        options.draftProvider.auth.loginUsername.trim() &&
          options.draftProvider.auth.loginPassword.trim(),
      );
    }
    return Boolean(options.draftProvider.auth.accessToken.trim() && options.draftProvider.auth.apiUser.trim());
  });

  watch(
    () => [
      options.draftProvider.identity.baseUrl,
      options.draftProvider.auth.mode,
      options.draftProvider.auth.sessionCookie,
      options.draftProvider.auth.accessToken,
      options.draftProvider.auth.apiUser,
      options.draftProvider.auth.apiKey,
      options.draftProvider.auth.loginUsername,
      options.draftProvider.auth.loginPassword,
    ],
    () => {
      if (!credentialAssistantBusy.value) {
        resetCredentialAssistant();
      }
    },
  );

  function resetCredentialAssistant() {
    credentialAssistantState.value = "idle";
    credentialAssistantSteps.value = [];
    credentialAssistantMessage.value = "";
    credentialAssistantChangedFields.value = [];
    credentialAssistantSaved.value = false;
  }

  async function probeSite(probeOptions: { silent?: boolean } = {}) {
    const silent = probeOptions.silent === true;
    if (options.probingSite.value) {
      return options.siteProbeResult.value;
    }
    if (!options.draftProvider.identity.baseUrl.trim()) {
      if (!silent) {
        Message.warning("请先填写中转站地址");
      }
      return;
    }

    const probingBaseUrl = options.draftProvider.identity.baseUrl;
    options.probingSite.value = true;
    options.siteProbeResult.value = null;
    try {
      const result = await options.probeProviderSite({
        ...options.draftProvider,
        id: options.editingProviderId.value ?? undefined,
      });
      options.siteProbeResult.value = result;
      if (result.systemName) {
        options.draftProvider.identity.name = result.systemName;
        options.siteNameSourceBaseUrl.value = normalizeProviderBaseUrl(probingBaseUrl);
      }
      if (silent) {
        return result;
      }
      if (result.ok) {
        Message.success(result.message);
      } else {
        Message.warning(result.message || "站点探测失败");
      }
      return result;
    } catch (error) {
      if (!silent) {
        Message.error(error instanceof Error ? error.message : String(error));
      }
      return null;
    } finally {
      options.probingSite.value = false;
    }
  }

  async function completeCredentials(runOptions: CompletionRunOptions = {}) {
    const notify = runOptions.notify !== false;
    const save = runOptions.save !== false;

    if (!options.draftProvider.identity.baseUrl.trim()) {
      if (notify) {
        Message.warning("请先填写中转站地址");
      }
      return;
    }

    options.completingCredentials.value = true;
    options.credentialCompletionMessage.value = "";
    options.credentialCompletionSteps.value = [];
    try {
      const result = await options.completeProviderCredentials({
        ...options.draftProvider,
        id: options.editingProviderId.value ?? undefined,
      });

      const apiKeyStep = result.steps.find((step) => step.name.includes("API 密钥"));
      const apiKeyQueryFailed = Boolean(
        apiKeyStep &&
          !apiKeyStep.ok &&
          !apiKeyStep.message.includes("站点没有已有 API Key"),
      );
      Object.assign(options.draftProvider, result.input);
      options.setApiKeyOptions(
        apiKeyQueryFailed ? result.input.auth.apiKeyOptions : result.apiKeyOptions,
      );
      options.credentialCompletionSteps.value = result.steps;
      if (result.changedFields.length > 0 || (!apiKeyQueryFailed && result.apiKeyOptions.length > 0)) {
        const changedLabels = result.changedFields.map(fieldLabel);
        options.credentialCompletionMessage.value = changedLabels.length > 0
          ? `已补全：${changedLabels.join("、")}`
          : `已同步 ${result.apiKeyOptions.length} 个 API Key`;
        if (save) {
          const savedProvider = await options.saveDraftAndFindProvider();
          options.refreshAfterSave(savedProvider);
        }
        if (notify) {
          Message.success(
            save
              ? `${options.credentialCompletionMessage.value}，已自动保存`
              : options.credentialCompletionMessage.value,
          );
        }
      } else {
        options.credentialCompletionMessage.value = "没有需要补全的凭据";
        if (notify) {
          Message.info(options.credentialCompletionMessage.value);
        }
      }
      return result;
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      options.credentialCompletionMessage.value = message;
      if (notify) {
        Message.error(message);
      }
      return null;
    } finally {
      options.completingCredentials.value = false;
    }
  }

  async function runCredentialAssistant() {
    if (!validateAssistantStart()) {
      return;
    }

    resetCredentialAssistant();
    setAssistantStep("site", "读取站点信息", "running", "正在读取站点名称和基础能力");
    credentialAssistantState.value = "probingSite";

    const site = await probeSite({ silent: true });
    if (!site) {
      failAssistantStep("site", "读取站点信息失败");
      return;
    }
    if (!site.ok) {
      failAssistantStep("site", site.message || "读取站点信息失败");
      return;
    }
    setAssistantStep("site", "读取站点信息", "done", site.message || "已读取站点信息");

    credentialAssistantState.value = "resolvingCredentials";
    setAssistantStep("credentials", "解析基础凭据", "running", "正在解析用户信息和已有凭据");
    const completion = await completeCredentials({ notify: false, save: false });
    if (!completion) {
      failAssistantStep("credentials", options.credentialCompletionMessage.value || "解析基础凭据失败");
      return;
    }
    const changedFields = completion.changedFields.map(fieldLabel);
    credentialAssistantChangedFields.value = changedFields;
    setAssistantStep(
      "credentials",
      "解析基础凭据",
      "done",
      changedFields.length > 0 ? `已补全：${changedFields.join("、")}` : "没有需要补全的基础凭据",
    );

    if (!(await ensureAccessToken())) {
      return;
    }

    if (!(await ensureApiKey())) {
      return;
    }

    await finishAssistantSave();
  }

  async function ensureAccessToken() {
    const canGenerateFromSession = ["session", "password"].includes(options.draftProvider.auth.mode);
    if (!canGenerateFromSession || options.draftProvider.auth.accessToken.trim()) {
      if (canGenerateFromSession) {
        setAssistantStep("accessToken", "生成访问令牌", "skipped", "已存在访问令牌");
      }
      return true;
    }
    if (!options.draftProvider.auth.sessionCookie.trim() || !options.draftProvider.auth.apiUser.trim()) {
      failAssistantStep("accessToken", "缺少会话 Cookie 或 API User ID，无法生成访问令牌");
      return false;
    }

    credentialAssistantState.value = "needAccessTokenConfirm";
    setAssistantStep("accessToken", "生成访问令牌", "running", "等待确认是否生成访问令牌");
    const confirmed = await confirmAction(
      "生成访问令牌",
      "当前中转站没有可用访问令牌。是否使用会话 Cookie 生成新的访问令牌？生成后可能覆盖该账号原有访问令牌。",
      "生成",
      "warning",
    );
    if (!confirmed) {
      setAssistantStep("accessToken", "生成访问令牌", "skipped", "已取消生成，保留当前认证方式");
      return true;
    }

    credentialAssistantState.value = "generatingAccessToken";
    setAssistantStep("accessToken", "生成访问令牌", "running", "正在生成访问令牌");
    options.completingCredentials.value = true;
    try {
      options.draftProvider.auth.accessToken = await options.generateAccessTokenForInput({
        ...options.draftProvider,
        id: options.editingProviderId.value ?? undefined,
      });
      setAssistantStep("accessToken", "生成访问令牌", "done", "访问令牌已生成");
      Message.success("访问令牌已生成");
      return true;
    } catch (error) {
      setAssistantStep(
        "accessToken",
        "生成访问令牌",
        "skipped",
        `生成失败，保留当前认证方式：${error instanceof Error ? error.message : String(error)}`,
      );
      return true;
    } finally {
      options.completingCredentials.value = false;
    }
  }

  async function ensureApiKey() {
    const apiKeyStep = options.credentialCompletionSteps.value.find((step) =>
      step.name.includes("API 密钥"),
    );
    if (
      !options.draftProvider.auth.apiKey.trim() &&
      apiKeyStep &&
      !apiKeyStep.ok &&
      !apiKeyStep.message.includes("站点没有已有 API Key")
    ) {
      failAssistantStep("apiKey", `未确认站点的 API Key 列表：${apiKeyStep.message}`);
      return false;
    }
    const knownKeys = options.draftProvider.auth.apiKeyOptions.filter(
      (option) => option.keyAvailable && option.key.trim(),
    );
    if (options.draftProvider.auth.apiKey.trim()) {
      if (options.draftProvider.auth.mode !== "apiKey") {
        setAssistantStep("apiKey", "同步 API 密钥", "done", "已同步并保留当前主 Key");
      }
      return true;
    }
    if (knownKeys.length === 1) {
      const option = knownKeys[0];
      options.draftProvider.auth.apiKey = option.key;
      options.draftProvider.auth.apiKeyTokenId = option.tokenId;
      setAssistantStep("apiKey", "选择主 API Key", "done", `已自动选择：${option.name || "未命名 Key"}`);
      return true;
    }
    if (knownKeys.length > 1) {
      credentialAssistantState.value = "needApiKeySelection";
      credentialAssistantMessage.value = "已发现多个 API Key，请先选择一个作为主 Key";
      setAssistantStep(
        "apiKey",
        "选择主 API Key",
        "pending",
        `已发现 ${knownKeys.length} 个可用 Key，请在上方列表中选择后继续保存`,
      );
      return false;
    }
    if (options.draftProvider.auth.apiKeyOptions.length > 0) {
      failAssistantStep("apiKey", "站点已有 API Key，但当前凭据无法读取完整 Key，未自动创建新 Key");
      return false;
    }
    if (!options.draftProvider.auth.apiUser.trim()) {
      failAssistantStep("apiKey", "缺少 API User ID，无法创建 API 密钥");
      return false;
    }
    if (!options.draftProvider.auth.sessionCookie.trim() && !options.draftProvider.auth.accessToken.trim()) {
      failAssistantStep("apiKey", "缺少会话 Cookie 或访问令牌，无法创建 API 密钥");
      return false;
    }

    credentialAssistantState.value = "needApiKeyName";
    setAssistantStep("apiKey", "创建 API 密钥", "running", "等待输入 API 密钥名称");
    const name = await promptApiKeyName();
    if (!name) {
      setAssistantStep("apiKey", "创建 API 密钥", "skipped", "已取消创建，保留当前认证方式");
      return true;
    }

    credentialAssistantState.value = "creatingApiKey";
    setAssistantStep("apiKey", "创建 API 密钥", "running", "正在创建 API 密钥");
    options.completingCredentials.value = true;
    try {
      const option = await options.createApiKeyForInput(
        {
          ...options.draftProvider,
          id: options.editingProviderId.value ?? undefined,
        },
        name,
      );
      options.draftProvider.auth.apiKey = option.key;
      options.draftProvider.auth.apiKeyTokenId = option.tokenId;
      options.setApiKeyOptions([...options.draftProvider.auth.apiKeyOptions, option]);
      setAssistantStep("apiKey", "创建 API 密钥", "done", `API 密钥已创建：${option.name || name}`);
      Message.success("API 密钥已创建");
      return true;
    } catch (error) {
      failAssistantStep("apiKey", `创建 API 密钥失败：${error instanceof Error ? error.message : String(error)}`);
      return false;
    } finally {
      options.completingCredentials.value = false;
    }
  }

  async function selectCredentialApiKey(option: ProviderApiKeyOption) {
    if (!option.keyAvailable || !option.key.trim()) {
      Message.warning("该 API Key 未读取到完整值，无法设为主 Key");
      return;
    }
    const resume = credentialAssistantState.value === "needApiKeySelection";
    if (resume) {
      // 先进入忙碌态，避免 draft 变化触发 watcher 清空当前助手步骤。
      credentialAssistantState.value = "saving";
    }
    options.draftProvider.auth.apiKey = option.key;
    options.draftProvider.auth.apiKeyTokenId = option.tokenId;
    options.setApiKeyOptions(options.draftProvider.auth.apiKeyOptions);
    setAssistantStep("apiKey", "选择主 API Key", "done", `已选择：${option.name || "未命名 Key"}`);
    if (resume) {
      await finishAssistantSave();
    }
  }

  async function finishAssistantSave() {
    const blockingFailures = unresolvedCompletionFailures();
    if (blockingFailures.length > 0) {
      failAssistantStep("credentials", blockingFailures.map((step) => step.message).join("；"));
      return;
    }

    credentialAssistantState.value = "saving";
    setAssistantStep("save", "保存配置", "running", "正在保存中转站配置");
    try {
      const savedProvider = await options.saveDraftAndFindProvider();
      options.refreshAfterSave(savedProvider);
      credentialAssistantSaved.value = true;
      credentialAssistantState.value = "done";
      credentialAssistantMessage.value = "配置已完成并保存";
      setAssistantStep("save", "保存配置", "done", "已保存，你可以继续调整高级配置");
      Message.success("配置已完成并保存");
    } catch (error) {
      failAssistantStep("save", error instanceof Error ? error.message : String(error));
    }
  }

  function unresolvedCompletionFailures() {
    return options.credentialCompletionSteps.value.filter((step) => {
      if (step.ok) {
        return false;
      }
      // Access tokens and API Keys are optional downstream credentials. A
      // user may keep a valid Cookie or account-password primary credential
      // after declining their generation; only an actual list-query failure
      // is handled as blocking by ensureApiKey above.
      if (step.name.includes("访问令牌") || step.name.includes("API 密钥")) {
        return false;
      }
      return true;
    });
  }

  function validateAssistantStart() {
    if (options.draftProvider.auth.mode === "apiKey") {
      Message.info("API 密钥模式不需要自动补全");
      return false;
    }
    if (!options.draftProvider.identity.baseUrl.trim()) {
      Message.warning("请先填写中转站地址");
      return false;
    }
    if (options.draftProvider.auth.mode === "session" && !options.draftProvider.auth.sessionCookie.trim()) {
      Message.warning("请先填写会话 Cookie");
      return false;
    }
    if (
      options.draftProvider.auth.mode === "password" &&
      (!options.draftProvider.auth.loginUsername.trim() ||
        !options.draftProvider.auth.loginPassword.trim())
    ) {
      Message.warning("请先填写账号和密码");
      return false;
    }
    if (
      options.draftProvider.auth.mode === "accessToken" &&
      (!options.draftProvider.auth.accessToken.trim() || !options.draftProvider.auth.apiUser.trim())
    ) {
      Message.warning("请先填写访问令牌和 API User ID");
      return false;
    }
    return true;
  }

  function setAssistantStep(
    key: string,
    name: string,
    status: CredentialCompletionStep["status"],
    message: string,
  ) {
    const index = credentialAssistantSteps.value.findIndex((step) => step.key === key);
    const nextStep = { key, name, status, message };
    if (index >= 0) {
      credentialAssistantSteps.value.splice(index, 1, nextStep);
    } else {
      credentialAssistantSteps.value.push(nextStep);
    }
  }

  function failAssistantStep(key: string, message: string) {
    const existing = credentialAssistantSteps.value.find((step) => step.key === key);
    setAssistantStep(key, existing?.name || "配置步骤", "error", message);
    credentialAssistantState.value = "failed";
    credentialAssistantMessage.value = message;
    credentialAssistantSaved.value = false;
    Message.error(message);
  }

  function confirmAction(
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

  function promptApiKeyName() {
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

  return {
    probeSite,
    completeCredentials,
    runCredentialAssistant,
    resetCredentialAssistant,
    canRunCredentialAssistant,
    credentialAssistantBusy,
    credentialAssistantState,
    credentialAssistantSteps,
    credentialAssistantMessage,
    credentialAssistantChangedFields,
    credentialAssistantSaved,
    selectCredentialApiKey,
  };
}
