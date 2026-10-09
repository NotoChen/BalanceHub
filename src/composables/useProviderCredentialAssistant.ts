import { computed, onScopeDispose, ref } from "vue";
import { Message } from "@arco-design/web-vue";
import type { ProviderApiKeyOption } from "../stores/providers";
import { chooseProviderApiKey, confirmAction } from "./provider-credential-dialogs";
import { openApiKeyEditor } from "./provider-api-key-editor";
import { withTimeout } from "../utils/promise-timeout";
import {
  blockingCredentialCompletionFailures,
  canRunCredentialAssistantForInput,
  canSkipAssistantAccessToken,
  credentialFieldHasValue,
  isEmptyApiKeyMessage,
  missingCredentialRequirements,
  needsCredentialCompletionForInput,
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

  let activeKeyEditor: ReturnType<typeof openApiKeyEditor<ProviderApiKeyOption>> | null = null;
  let interactionController = new AbortController();
  let operationRevision = 0;
  let disposed = false;
  onScopeDispose(() => { disposed = true; resetCredentialAssistant(); });

  const credentialAssistantState = ref<CredentialCompletionState>("idle");
  const credentialAssistantSteps = ref<CredentialCompletionStep[]>([]);
  const credentialAssistantMessage = ref("");
  const credentialAssistantChangedFields = ref<string[]>([]);
  const credentialAssistantSaved = ref(false);
  const credentialAssistantBusy = computed(() =>
    [
      "probingSite",
      "resolvingCredentials",
      "needAccessTokenConfirm",
      "generatingAccessToken",
      "needApiKeySelection",
      "needApiKeySettings",
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
    operationRevision += 1;
    interactionController.abort();
    interactionController = new AbortController();
    activeKeyEditor?.close();
    activeKeyEditor = null;
    options.completingCredentials.value = false;
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
    const revision = operationRevision;
    const current = () => !disposed && revision === operationRevision && requestContextIsCurrent(requestContext);
    options.completingCredentials.value = true;
    options.credentialCompletionMessage.value = "";
    options.credentialCompletionSteps.value = [];
    try {
      const result = await withTimeout(options.completeProviderCredentials(requestInput), 60_000, "自动补全凭据超时，当前填写内容已保留，请重试");
      if (!current()) {
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
      if (!current()) {
        return null;
      }
      options.credentialCompletionMessage.value = message;
      if (notify) {
        Message.error(message);
      }
      return null;
    } finally {
      if (revision === operationRevision && editorSessionIsActive(requestContext)) {
        options.completingCredentials.value = false;
      }
    }
  }

  async function prepareCredentialsForSave() {
    const prepared = await runCredentialAssistant({ save: false, onlyMissing: true });
    if (!prepared && credentialAssistantState.value === "failed") {
      throw new Error(credentialAssistantMessage.value || "自动补全未完成，当前填写内容已保留");
    }
    return prepared;
  }

  async function runCredentialAssistant(runOptions: { save?: boolean; onlyMissing?: boolean } = {}) {
    if (disposed || credentialAssistantBusy.value) return false;
    if (runOptions.onlyMissing && !needsCredentialCompletionForInput(options.draftProvider, options.providerProtocols())) return true;
    if (!validateAssistantStart()) return false;
    resetCredentialAssistant();
    const revision = operationRevision;
    const assistantContext = captureRequestContext();
    const current = () => !disposed && revision === operationRevision && editorSessionIsActive(assistantContext);
    try {
      if (!runOptions.onlyMissing) {
        setAssistantStep("site", "读取站点信息", "running", "正在读取站点名称和基础能力");
        credentialAssistantState.value = "probingSite";
        const siteContext = captureRequestContext();
        const site = await probeSite({ silent: true });
        if (!current()) return false;
        if (!site) {
          if (requestContextIsCurrent(siteContext)) failAssistantStep("site", "读取站点信息失败");
          return false;
        }
        if (!site.ok) {
          failAssistantStep("site", site.message || "读取站点信息失败");
          return false;
        }
        setAssistantStep("site", "读取站点信息", "done", site.message || "已读取站点信息");
      }
      credentialAssistantState.value = "resolvingCredentials";
      setAssistantStep("credentials", "补全认证凭据", "running", "正在读取用户信息、登录凭据和 API Key");
      const completionContext = captureRequestContext();
      const completion = await completeCredentials({ notify: false, save: false });
      if (!current()) return false;
      if (!completion) {
        if (requestContextIsCurrent(completionContext)) failAssistantStep("credentials", options.credentialCompletionMessage.value || "补全认证凭据失败");
        return false;
      }
      const blockingFailures = blockingCredentialCompletionFailures(completion.steps);
      if (blockingFailures.length > 0) {
        failAssistantStep("credentials", blockingFailures.map((step) => step.message).join("；"));
        return false;
      }
      const changedFields = completion.changedFields.map(fieldLabel);
      credentialAssistantChangedFields.value = changedFields;
      setAssistantStep("credentials", "补全认证凭据", "done", changedFields.length > 0 ? `已补全：${changedFields.join("、")}` : "已读取现有凭据");
      if (!(await ensureAccessToken()) || !current()) return false;
      if (!(await ensureApiKey()) || !current()) return false;
      if (runOptions.save === false) {
        credentialAssistantState.value = "done";
        credentialAssistantMessage.value = "凭据检查完成，正在保存";
        return true;
      }
      await finishAssistantSave();
      return credentialAssistantSaved.value;
    } catch (error) {
      if (current()) failAssistantStep("credentials", error instanceof Error ? error.message : String(error));
      return false;
    } finally {
      if (revision === operationRevision) {
        options.completingCredentials.value = false;
        if (credentialAssistantBusy.value) credentialAssistantState.value = "idle";
      }
    }
  }

  async function ensureAccessToken() {
    const protocol = currentProtocolDescriptor();
    const flow = protocol?.credentialAssistant.accessTokenFlow ?? "none";
    if (flow === "none") {
      setAssistantStep("accessToken", "获取访问令牌", "skipped", "当前协议不需要访问令牌");
      return true;
    }
    if (protocol && canSkipAssistantAccessToken(options.draftProvider, protocol)) {
      setAssistantStep("accessToken", "获取访问令牌", "skipped", "已有可续期的登录会话，无需额外生成访问令牌");
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
    const revision = operationRevision;
    const confirmed = await confirmAction(
      "生成访问令牌",
      "当前中转站没有可用访问令牌。是否使用会话 Cookie 生成新的访问令牌？生成后可能覆盖该账号原有访问令牌。",
      "生成",
      "warning",
      interactionController.signal,
    );
    if (revision !== operationRevision || !requestContextIsCurrent(confirmationContext)) {
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
      const accessToken = await withTimeout(options.generateAccessTokenForInput(requestInput), 30_000, "生成访问令牌超时，请先在站点确认生成结果");
      if (revision !== operationRevision || !requestContextIsCurrent(requestContext)) {
        return false;
      }
      options.draftProvider.auth.accessToken = accessToken;
      setAssistantStep("accessToken", "生成访问令牌", "done", "访问令牌已生成");
      Message.success("访问令牌已生成");
      return true;
    } catch (error) {
      if (revision !== operationRevision || !requestContextIsCurrent(requestContext)) {
        return false;
      }
      failAssistantStep("accessToken", `生成访问令牌失败：${error instanceof Error ? error.message : String(error)}`);
      return false;
    } finally {
      if (revision === operationRevision && editorSessionIsCurrent(requestContext)) {
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
      setAssistantStep("apiKey", "选择当前调用 API Key", "running", `已读取 ${knownKeys.length} 把 Key，等待选择`);
      const context = captureRequestContext();
      const revision = operationRevision;
      const option = await chooseProviderApiKey(knownKeys, interactionController.signal);
      if (revision !== operationRevision || !requestContextIsCurrent(context)) return false;
      if (!option) {
        credentialAssistantState.value = "idle";
        setAssistantStep("apiKey", "选择当前调用 API Key", "pending", "已取消选择，当前填写内容已保留");
        return false;
      }
      options.draftProvider.auth.apiKey = option.key;
      options.draftProvider.auth.apiKeyTokenId = option.tokenId;
      setAssistantStep("apiKey", "选择当前调用 API Key", "done", `已选择：${providerApiKeyDisplayName(option)}`);
      return true;
    }
    if (options.draftProvider.auth.apiKeyOptions.length > 0) {
      failAssistantStep("apiKey", "站点已有 API Key，但当前凭据无法读取完整 Key，未自动创建新 Key");
      return false;
    }
    const requiredFields = protocol.credentialAssistant.apiKeyRequiredFields.filter(
      (field) => !credentialFieldHasValue(options.draftProvider, field),
    );
    if (requiredFields.length > 0) {
      failAssistantStep(
        "apiKey",
        `缺少${requiredFields.map(authFieldLabel).join("、")}，无法创建 API 密钥`,
      );
      return false;
    }
    const anyFields = protocol.credentialAssistant.apiKeyRequiredAnyFields;
    if (anyFields.length > 0 && !anyFields.some((field) => credentialFieldHasValue(options.draftProvider, field))) {
      failAssistantStep(
        "apiKey",
        `至少需要${anyFields.map(authFieldLabel).join("或")}，无法创建 API 密钥`,
      );
      return false;
    }

    if (!options.editingProviderId.value) {
      credentialAssistantState.value = "saving";
      setAssistantStep("save", "保存账号配置", "running", "正在保存账号，以便创建和管理站点 Key");
      const context = captureRequestContext();
      const saved = await options.saveDraftAndFindProvider(() => requestContextIsCurrent(context));
      if (!saved) return false;
      options.editingProviderId.value = saved.identity.id;
      options.draftProvider.id = saved.identity.id;
      Object.assign(options.draftProvider.auth, saved.auth);
      setAssistantStep("save", "保存账号配置", "done", "账号已保存，继续设置 API Key");
    }
    credentialAssistantState.value = "needApiKeySettings";
    setAssistantStep("apiKey", "创建 API 密钥", "running", "等待填写 Key 设置");
    const requestInput = snapshotInput();
    const requestContext = captureRequestContext(requestInput);
    const editor = openApiKeyEditor({
      editing: false,
      loadContext: () => options.apiKeyEditorContextForInput(requestInput),
      submit: (patch) => {
        if (!requestContextIsCurrent(requestContext)) return Promise.reject(new Error("账号配置已变更，请重新打开 Key 设置"));
        return options.createApiKeyForInput(requestInput, patch);
      },
    });
    activeKeyEditor = editor;
    const option = await editor.result;
    if (activeKeyEditor === editor) activeKeyEditor = null;
    if (!requestContextIsCurrent(requestContext)) return false;
    if (!option) {
      setAssistantStep("apiKey", "创建 API 密钥", "skipped", "已取消创建，保留当前认证方式");
      return true;
    }
    credentialAssistantState.value = "saving";
    if (option.keyAvailable) {
      options.draftProvider.auth.apiKey = option.key;
      options.draftProvider.auth.apiKeyTokenId = option.tokenId;
    }
    options.setApiKeyOptions([...options.draftProvider.auth.apiKeyOptions, option]);
    setAssistantStep("apiKey", "创建 API 密钥", "done", `API 密钥已创建：${providerApiKeyDisplayName(option)}`);
    if (!option.keyAvailable) {
      failAssistantStep("apiKey", "Key 已创建，但站点未返回完整密钥值，请同步站点 Key 后确认");
      return false;
    }
    Message.success("API 密钥已创建");
    return true;
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
    if (!schema) {
      Message.warning("认证信息尚未加载，请重新打开编辑窗口");
      return false;
    }
    const missingFields = missingCredentialRequirements(options.draftProvider, schema)
      .map((fields) => fields.map(authFieldLabel).join("或"));
    if (missingFields.length > 0) {
      Message.warning(`请先填写${missingFields.join("、")}`);
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
    prepareCredentialsForSave,
    resetCredentialAssistant,
    canRunCredentialAssistant,
    credentialAssistantBusy,
    credentialAssistantState,
    credentialAssistantSteps,
    credentialAssistantMessage,
    credentialAssistantChangedFields,
    credentialAssistantSaved,
  };
}
