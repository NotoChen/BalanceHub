import { computed, ref } from "vue";
import { Message } from "@arco-design/web-vue";
import type { ProviderApiKeyOption, ProviderInput } from "../stores/providers";
import { confirmAction, promptApiKeyName } from "./provider-credential-dialogs";
import {
  blockingCredentialCompletionFailures,
  canRunCredentialAssistantForInput,
  isEmptyApiKeyMessage,
} from "./provider-credential-rules";
import { fieldLabel } from "./provider-editor-shared";
import {
  providerAuthModeDescriptor,
  providerProtocolDescriptor,
} from "../utils/provider-protocol";
import { providerApiKeyDisplayName } from "../utils/provider-display";
import type {
  CompletionRunOptions,
  CredentialCompletionState,
  CredentialCompletionStep,
  ProviderCredentialRequestGuard,
  ProviderSiteProbe,
  UseProviderCredentialCompletionOptions,
} from "./provider-credential-types";

export function useProviderCredentialAssistant(
  options: UseProviderCredentialCompletionOptions,
  requestGuard: ProviderCredentialRequestGuard,
  probeSite: ProviderSiteProbe,
) {
  const {
    snapshotInput,
    captureRequestContext,
    editorSessionIsActive,
    editorSessionIsCurrent,
    requestContextIsCurrent,
  } = requestGuard;

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

  const canRunCredentialAssistant = computed(() =>
    canRunCredentialAssistantForInput(
      options.draftProvider,
      options.providerProtocols(),
      credentialAssistantBusy.value,
    ),
  );

  function resetCredentialAssistant() {
    credentialAssistantState.value = "idle";
    credentialAssistantSteps.value = [];
    credentialAssistantMessage.value = "";
    credentialAssistantChangedFields.value = [];
    credentialAssistantSaved.value = false;
  }

  function currentProtocolDescriptor() {
    return providerProtocolDescriptor(
      options.providerProtocols(),
      options.draftProvider.identity.protocol,
    );
  }

  function currentAuthModeDescriptor() {
    return providerAuthModeDescriptor(
      options.providerProtocols(),
      options.draftProvider.identity.protocol,
      options.draftProvider.auth.mode,
    );
  }

  function authFieldValue(field: string) {
    const value = options.draftProvider.auth[field as keyof ProviderInput["auth"]];
    return typeof value === "string" ? value : "";
  }

  function authFieldLabel(field: string) {
    const protocols = options.providerProtocols();
    for (const protocol of protocols) {
      for (const mode of protocol.authModes) {
        const descriptor = mode.fields.find((candidate) => candidate.field === field);
        if (descriptor) return descriptor.label;
      }
    }
    return fieldLabel(field);
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

    const requestInput = snapshotInput();
    const requestContext = captureRequestContext(requestInput);
    options.completingCredentials.value = true;
    options.credentialCompletionMessage.value = "";
    options.credentialCompletionSteps.value = [];
    try {
      const result = await options.completeProviderCredentials(requestInput);
      if (!requestContextIsCurrent(requestContext)) {
        return null;
      }

      const apiKeyStep = result.steps.find(
        (step) => step.name.includes("API 密钥") || step.name.includes("API Key"),
      );
      const apiKeyQueryFailed = Boolean(
        apiKeyStep &&
          !apiKeyStep.ok &&
          !isEmptyApiKeyMessage(apiKeyStep.message),
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
          const saveContext = captureRequestContext();
          const savedProvider = await options.saveDraftAndFindProvider(
            () => requestContextIsCurrent(saveContext),
          );
          if (!savedProvider) {
            return null;
          }
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
      if (!requestContextIsCurrent(requestContext)) {
        return null;
      }
      options.credentialCompletionMessage.value = message;
      if (notify) {
        Message.error(message);
      }
      return null;
    } finally {
      if (editorSessionIsActive(requestContext)) {
        options.completingCredentials.value = false;
      }
    }
  }

  async function runCredentialAssistant() {
    if (!validateAssistantStart()) {
      return;
    }

    const assistantContext = captureRequestContext();
    resetCredentialAssistant();
    setAssistantStep("site", "读取站点信息", "running", "正在读取站点名称和基础能力");
    credentialAssistantState.value = "probingSite";

    const siteContext = captureRequestContext();
    const site = await probeSite({ silent: true });
    if (!editorSessionIsCurrent(assistantContext)) {
      return;
    }
    if (!site) {
      if (!requestContextIsCurrent(siteContext)) {
        resetCredentialAssistant();
        return;
      }
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
    const completionContext = captureRequestContext();
    const completion = await completeCredentials({ notify: false, save: false });
    if (!editorSessionIsCurrent(assistantContext)) {
      return;
    }
    if (!completion) {
      if (!requestContextIsCurrent(completionContext)) {
        resetCredentialAssistant();
        return;
      }
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

    const accessTokenContext = captureRequestContext();
    if (!(await ensureAccessToken())) {
      if (
        editorSessionIsCurrent(assistantContext)
        && !requestContextIsCurrent(accessTokenContext)
      ) {
        resetCredentialAssistant();
      }
      return;
    }
    if (!editorSessionIsCurrent(assistantContext)) {
      return;
    }

    const apiKeyContext = captureRequestContext();
    if (!(await ensureApiKey())) {
      if (
        editorSessionIsCurrent(assistantContext)
        && !requestContextIsCurrent(apiKeyContext)
      ) {
        resetCredentialAssistant();
      }
      return;
    }
    if (!editorSessionIsCurrent(assistantContext)) {
      return;
    }

    await finishAssistantSave();
  }

  async function ensureAccessToken() {
    const protocol = currentProtocolDescriptor();
    const flow = protocol?.credentialAssistant.accessTokenFlow ?? "none";
    if (flow === "none") {
      setAssistantStep("accessToken", "获取访问令牌", "skipped", "当前协议不需要访问令牌");
      return true;
    }
    if (flow === "credentialCompletion") {
      if (options.draftProvider.auth.accessToken.trim()) {
        setAssistantStep("accessToken", "获取访问令牌", "skipped", "访问令牌已存在");
        return true;
      }
      failAssistantStep("accessToken", `${protocol?.label || "当前协议"} 凭据补全没有返回访问令牌`);
      return false;
    }
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
    const confirmationContext = captureRequestContext();
    const confirmed = await confirmAction(
      "生成访问令牌",
      "当前中转站没有可用访问令牌。是否使用会话 Cookie 生成新的访问令牌？生成后可能覆盖该账号原有访问令牌。",
      "生成",
      "warning",
    );
    if (!requestContextIsCurrent(confirmationContext)) {
      return false;
    }
    if (!confirmed) {
      setAssistantStep("accessToken", "生成访问令牌", "skipped", "已取消生成，保留当前认证方式");
      return true;
    }

    credentialAssistantState.value = "generatingAccessToken";
    setAssistantStep("accessToken", "生成访问令牌", "running", "正在生成访问令牌");
    options.completingCredentials.value = true;
    const requestInput = snapshotInput();
    const requestContext = captureRequestContext(requestInput);
    try {
      const accessToken = await options.generateAccessTokenForInput(requestInput);
      if (!requestContextIsCurrent(requestContext)) {
        return false;
      }
      options.draftProvider.auth.accessToken = accessToken;
      setAssistantStep("accessToken", "生成访问令牌", "done", "访问令牌已生成");
      Message.success("访问令牌已生成");
      return true;
    } catch (error) {
      if (!requestContextIsCurrent(requestContext)) {
        return false;
      }
      setAssistantStep(
        "accessToken",
        "生成访问令牌",
        "skipped",
        `生成失败，保留当前认证方式：${error instanceof Error ? error.message : String(error)}`,
      );
      return true;
    } finally {
      if (editorSessionIsCurrent(requestContext)) {
        options.completingCredentials.value = false;
      }
    }
  }

  async function ensureApiKey() {
    const protocol = currentProtocolDescriptor();
    if (!protocol?.capabilities.apiKeyManagement) {
      setAssistantStep("apiKey", "同步 API 密钥", "skipped", "当前协议不提供 API Key 管理能力");
      return true;
    }
    const apiKeyStep = options.credentialCompletionSteps.value.find((step) =>
      step.name.includes("API 密钥") || step.name.includes("API Key"),
    );
    if (
      !options.draftProvider.auth.apiKey.trim() &&
      apiKeyStep &&
      !apiKeyStep.ok &&
      !isEmptyApiKeyMessage(apiKeyStep.message)
    ) {
      failAssistantStep("apiKey", `未确认站点的 API Key 列表：${apiKeyStep.message}`);
      return false;
    }
    const knownKeys = options.draftProvider.auth.apiKeyOptions.filter(
      (option) => option.keyAvailable && option.key.trim(),
    );
    if (options.draftProvider.auth.apiKey.trim()) {
      if (options.draftProvider.auth.mode !== "apiKey") {
        setAssistantStep("apiKey", "同步 API 密钥", "done", "已同步并保留当前调用 Key");
      }
      return true;
    }
    if (knownKeys.length === 1) {
      const option = knownKeys[0];
      options.draftProvider.auth.apiKey = option.key;
      options.draftProvider.auth.apiKeyTokenId = option.tokenId;
      setAssistantStep(
        "apiKey",
        "选择当前调用 API Key",
        "done",
        `已自动选择：${providerApiKeyDisplayName(option)}`,
      );
      return true;
    }
    if (knownKeys.length > 1) {
      credentialAssistantState.value = "needApiKeySelection";
      credentialAssistantMessage.value = "已发现多个 API Key，请先选择本卡片用于默认请求的 Key";
      setAssistantStep(
        "apiKey",
        "选择当前调用 API Key",
        "pending",
        `已发现 ${knownKeys.length} 个可用 Key，请在上方列表中选择后继续保存`,
      );
      return false;
    }
    if (options.draftProvider.auth.apiKeyOptions.length > 0) {
      failAssistantStep("apiKey", "站点已有 API Key，但当前凭据无法读取完整 Key，未自动创建新 Key");
      return false;
    }
    const requiredFields = protocol.credentialAssistant.apiKeyRequiredFields.filter(
      (field) => !authFieldValue(field).trim(),
    );
    if (requiredFields.length > 0) {
      failAssistantStep(
        "apiKey",
        `缺少${requiredFields.map(authFieldLabel).join("、")}，无法创建 API 密钥`,
      );
      return false;
    }
    const anyFields = protocol.credentialAssistant.apiKeyRequiredAnyFields;
    if (anyFields.length > 0 && !anyFields.some((field) => authFieldValue(field).trim())) {
      failAssistantStep(
        "apiKey",
        `至少需要${anyFields.map(authFieldLabel).join("或")}，无法创建 API 密钥`,
      );
      return false;
    }

    credentialAssistantState.value = "needApiKeyName";
    setAssistantStep("apiKey", "创建 API 密钥", "running", "等待输入 API 密钥名称");
    const promptContext = captureRequestContext();
    const name = await promptApiKeyName();
    if (!requestContextIsCurrent(promptContext)) {
      return false;
    }
    if (!name) {
      setAssistantStep("apiKey", "创建 API 密钥", "skipped", "已取消创建，保留当前认证方式");
      return true;
    }

    credentialAssistantState.value = "creatingApiKey";
    setAssistantStep("apiKey", "创建 API 密钥", "running", "正在创建 API 密钥");
    options.completingCredentials.value = true;
    const requestInput = snapshotInput();
    const requestContext = captureRequestContext(requestInput);
    try {
      const option = await options.createApiKeyForInput(
        requestInput,
        name,
      );
      if (!requestContextIsCurrent(requestContext)) {
        return false;
      }
      options.draftProvider.auth.apiKey = option.key;
      options.draftProvider.auth.apiKeyTokenId = option.tokenId;
      options.setApiKeyOptions([...options.draftProvider.auth.apiKeyOptions, option]);
      setAssistantStep(
        "apiKey",
        "创建 API 密钥",
        "done",
        `API 密钥已创建：${providerApiKeyDisplayName(option) || name}`,
      );
      Message.success("API 密钥已创建");
      return true;
    } catch (error) {
      if (!requestContextIsCurrent(requestContext)) {
        return false;
      }
      failAssistantStep("apiKey", `创建 API 密钥失败：${error instanceof Error ? error.message : String(error)}`);
      return false;
    } finally {
      if (editorSessionIsCurrent(requestContext)) {
        options.completingCredentials.value = false;
      }
    }
  }

  async function selectCredentialApiKey(option: ProviderApiKeyOption) {
    if (!option.keyAvailable || !option.key.trim()) {
      Message.warning("该 API Key 未读取到完整值，无法设为当前调用 Key");
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
    setAssistantStep(
      "apiKey",
      "选择当前调用 API Key",
      "done",
      `已选择：${providerApiKeyDisplayName(option)}`,
    );
    if (resume) {
      await finishAssistantSave();
    }
  }

  async function finishAssistantSave() {
    const blockingFailures = blockingCredentialCompletionFailures(
      options.credentialCompletionSteps.value,
    );
    if (blockingFailures.length > 0) {
      failAssistantStep("credentials", blockingFailures.map((step) => step.message).join("；"));
      return;
    }

    credentialAssistantState.value = "saving";
    setAssistantStep("save", "保存配置", "running", "正在保存中转站配置");
    const saveContext = captureRequestContext();
    try {
      const savedProvider = await options.saveDraftAndFindProvider(
        () => requestContextIsCurrent(saveContext),
      );
      if (!savedProvider) {
        if (
          editorSessionIsCurrent(saveContext)
          && !requestContextIsCurrent(saveContext)
        ) {
          resetCredentialAssistant();
        }
        return;
      }
      options.refreshAfterSave(savedProvider);
      credentialAssistantSaved.value = true;
      credentialAssistantState.value = "done";
      credentialAssistantMessage.value = "配置已完成并保存";
      setAssistantStep("save", "保存配置", "done", "已保存，你可以继续调整高级配置");
      Message.success("配置已完成并保存");
    } catch (error) {
      if (!requestContextIsCurrent(saveContext)) {
        return;
      }
      failAssistantStep("save", error instanceof Error ? error.message : String(error));
    }
  }

  function validateAssistantStart() {
    if (options.draftProvider.auth.mode === "apiKey") {
      Message.info("API 密钥模式不需要自动补全");
      return false;
    }
    const protocol = currentProtocolDescriptor();
    if (!protocol?.credentialAssistant.enabled) {
      Message.info(`${protocol?.label || "当前协议"}不需要账号凭据补全`);
      return false;
    }
    if (!options.draftProvider.identity.baseUrl.trim()) {
      Message.warning("请先填写中转站地址");
      return false;
    }
    const schema = currentAuthModeDescriptor();
    const missingFields = schema?.requiredFields.filter(
      (field) => !authFieldValue(field).trim(),
    ) ?? [];
    if (missingFields.length > 0) {
      Message.warning(`请先填写${missingFields.map(authFieldLabel).join("、")}`);
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

  return {
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
