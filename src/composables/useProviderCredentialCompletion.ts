import { watch } from "vue";
import { Message } from "@arco-design/web-vue";
import type {
  ProviderInput,
  ProviderProtocol,
  ProviderSiteProbeResult,
} from "../stores/providers";
import {
  normalizeProviderBaseUrl,
  type ProtocolSelectionSource,
} from "./provider-editor-shared";
import { confirmAction } from "./provider-credential-dialogs";
import { useProviderCredentialAssistant } from "./useProviderCredentialAssistant";
import type {
  EditorRequestContext,
  ProviderCredentialRequestGuard,
  UseProviderCredentialCompletionOptions,
} from "./provider-credential-types";

export type {
  CredentialCompletionState,
  CredentialCompletionStep,
} from "./provider-credential-types";

export function useProviderCredentialCompletion(options: UseProviderCredentialCompletionOptions) {
  let activeSiteProbe: {
    key: string;
    request: Promise<ProviderSiteProbeResult | null | undefined>;
  } | null = null;
  let siteProbeRevision = 0;

  function snapshotInput(): ProviderInput {
    return JSON.parse(JSON.stringify({
      ...options.draftProvider,
      id: options.editingProviderId.value ?? undefined,
    })) as ProviderInput;
  }

  function captureRequestContext(input = snapshotInput()): EditorRequestContext {
    return {
      editorSession: options.editorSession.value,
      providerId: options.editingProviderId.value,
      inputFingerprint: JSON.stringify(input),
    };
  }

  function requestContextKey(context: EditorRequestContext) {
    return `${context.editorSession}:${context.providerId ?? "new"}:${context.inputFingerprint}`;
  }

  function editorSessionIsActive(context: EditorRequestContext) {
    return options.drawerVisible.value
      && options.editorSession.value === context.editorSession;
  }

  function editorSessionIsCurrent(context: EditorRequestContext) {
    return editorSessionIsActive(context)
      && options.editingProviderId.value === context.providerId;
  }

  function requestContextIsCurrent(context: EditorRequestContext) {
    return editorSessionIsCurrent(context)
      && JSON.stringify(snapshotInput()) === context.inputFingerprint;
  }

  const requestGuard: ProviderCredentialRequestGuard = {
    snapshotInput,
    captureRequestContext,
    editorSessionIsActive,
    editorSessionIsCurrent,
    requestContextIsCurrent,
  };
  const {
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
  } = useProviderCredentialAssistant(options, requestGuard, probeSite);

  watch(
    () => [
      options.draftProvider.identity.baseUrl,
      options.draftProvider.identity.protocol,
      options.draftProvider.auth.mode,
      options.draftProvider.auth.sessionCookie,
      options.draftProvider.auth.accessToken,
      options.draftProvider.auth.apiUser,
      options.draftProvider.auth.apiKey,
      options.draftProvider.auth.loginUsername,
      options.draftProvider.auth.loginPassword,
    ],
    () => {
      const currentBaseUrl = normalizeProviderBaseUrl(options.draftProvider.identity.baseUrl);
      if (currentBaseUrl !== options.protocolSelectionBaseUrl.value) {
        options.protocolSelectionSource.value = "auto";
        options.protocolDetectionResult.value = null;
        options.siteProbeResult.value = null;
      }
      if (!credentialAssistantBusy.value) {
        resetCredentialAssistant();
      }
    },
  );

  watch(
    () => [options.drawerVisible.value, options.editorSession.value] as const,
    ([visible]) => {
      siteProbeRevision += 1;
      activeSiteProbe = null;
      options.probingSite.value = false;
      if (!visible) {
        resetCredentialAssistant();
      }
    },
  );

  watch(
    () => options.draftProvider.auth.mode,
    (mode, previousMode) => {
      if (mode === "apiKey" && previousMode !== "apiKey") {
        void ensureProtocolSelection();
      }
    },
  );

  async function probeSite(
    probeOptions: { silent?: boolean; force?: boolean; skipDetection?: boolean } = {},
  ) {
    const input = snapshotInput();
    const context = captureRequestContext(input);
    const key = requestContextKey(context);
    if (activeSiteProbe?.key === key) {
      return activeSiteProbe.request;
    }

    const revision = ++siteProbeRevision;
    const request = runProbeSite(probeOptions, input, context, revision);
    activeSiteProbe = { key, request };
    try {
      return await request;
    } finally {
      if (activeSiteProbe?.request === request) {
        activeSiteProbe = null;
      }
    }
  }

  async function runProbeSite(
    probeOptions: { silent?: boolean; force?: boolean; skipDetection?: boolean } = {},
    initialInput: ProviderInput,
    initialContext: EditorRequestContext,
    revision: number,
  ) {
    const silent = probeOptions.silent === true;
    if (!options.draftProvider.identity.baseUrl.trim()) {
      if (!silent) {
        Message.warning("请先填写中转站地址");
      }
      return;
    }

    const probingBaseUrl = initialInput.identity.baseUrl;
    const normalizedBaseUrl = normalizeProviderBaseUrl(probingBaseUrl);
    const shouldDetect = !probeOptions.skipDetection && (
      probeOptions.force === true
      || (
        options.protocolSelectionSource.value === "auto"
        && options.protocolSelectionBaseUrl.value !== normalizedBaseUrl
      )
    );
    options.probingSite.value = true;
    options.siteProbeResult.value = null;
    try {
      if (shouldDetect) {
        const detection = await options.detectProviderProtocol(initialInput);
        if (!requestContextIsCurrent(initialContext)) {
          return null;
        }

        options.protocolDetectionResult.value = detection;
        options.protocolSelectionBaseUrl.value = normalizedBaseUrl;
        const detectedProtocol = detection.detectedProtocol;
        if (!detectedProtocol) {
          options.protocolSelectionSource.value = "unresolved";
          if (!silent) {
            Message.warning(detection.message);
          }
          return null;
        }

        const switched = await applyProtocolSelection(
          detectedProtocol,
          "auto",
          normalizedBaseUrl,
          initialContext,
        );
        if (!switched || !editorSessionIsCurrent(initialContext)) {
          return null;
        }

        const result = detection.site;
        if (result) {
          applySiteResult(result, probingBaseUrl);
          if (!silent) {
            Message.success(detection.message);
          }
          return result;
        }
      }

      const probeInput = snapshotInput();
      const probeContext = captureRequestContext(probeInput);
      const result = await options.probeProviderSite(probeInput);
      if (!requestContextIsCurrent(probeContext)) {
        return null;
      }
      applySiteResult(result, probingBaseUrl);
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
      if (
        !silent
        && revision === siteProbeRevision
        && editorSessionIsCurrent(initialContext)
      ) {
        Message.error(error instanceof Error ? error.message : String(error));
      }
      return null;
    } finally {
      if (
        revision === siteProbeRevision
        && editorSessionIsCurrent(initialContext)
      ) {
        options.probingSite.value = false;
      }
    }
  }

  async function selectProtocol(protocol: ProviderProtocol) {
    if (options.probingSite.value) {
      return;
    }
    const baseUrl = normalizeProviderBaseUrl(options.draftProvider.identity.baseUrl);
    if (options.draftProvider.identity.protocol === protocol) {
      options.protocolSelectionSource.value = "manual";
      options.protocolSelectionBaseUrl.value = baseUrl;
      return;
    }
    const switched = await applyProtocolSelection(protocol, "manual", baseUrl);
    if (!switched || !baseUrl) {
      return;
    }
    await probeSite({ silent: true, skipDetection: true });
  }

  async function ensureProtocolSelection() {
    if (activeSiteProbe) {
      const context = captureRequestContext();
      if (activeSiteProbe.key === requestContextKey(context)) {
        await activeSiteProbe.request;
      }
    }

    const baseUrl = normalizeProviderBaseUrl(options.draftProvider.identity.baseUrl);
    if (!baseUrl) {
      return;
    }

    const selectionSource = options.protocolSelectionSource.value;
    const shouldDetect = (
      selectionSource === "auto"
      && options.protocolSelectionBaseUrl.value !== baseUrl
    ) || (
      selectionSource === "unresolved"
      && options.draftProvider.auth.mode === "apiKey"
      && !options.protocolDetectionResult.value?.ambiguous
    );
    if (!shouldDetect) {
      return;
    }

    await probeSite({
      silent: true,
      force: selectionSource === "unresolved",
    });
  }

  async function applyProtocolSelection(
    protocol: ProviderProtocol,
    source: ProtocolSelectionSource,
    baseUrl: string,
    expectedContext = captureRequestContext(),
  ) {
    if (!requestContextIsCurrent(expectedContext)) {
      return false;
    }
    if (options.draftProvider.identity.protocol === protocol) {
      options.protocolSelectionSource.value = source;
      options.protocolSelectionBaseUrl.value = baseUrl;
      return true;
    }

    const hasProtocolCredentials = Boolean(
      options.draftProvider.auth.sessionCookie.trim()
      || options.draftProvider.auth.accessToken.trim()
      || options.draftProvider.auth.refreshToken.trim()
      || options.draftProvider.auth.apiUser.trim(),
    );
    if (hasProtocolCredentials) {
      const confirmed = await confirmAction(
        "切换中转站协议",
        `切换为 ${protocolLabel(protocol)} 后，当前 Cookie、访问令牌、刷新令牌和 API User ID 将被清空。账号密码和 API Key 会保留。`,
        "切换",
        "warning",
      );
      if (!requestContextIsCurrent(expectedContext)) {
        return false;
      }
      if (!confirmed) {
        options.protocolSelectionSource.value = "manual";
        options.protocolSelectionBaseUrl.value = baseUrl;
        return false;
      }
    }

    options.draftProvider.identity.protocol = protocol;
    options.draftProvider.auth.sessionCookie = "";
    options.draftProvider.auth.accessToken = "";
    options.draftProvider.auth.refreshToken = "";
    options.draftProvider.auth.accessTokenExpiresAt = null;
    options.draftProvider.auth.apiUser = "";
    options.draftProvider.auth.apiKeyTokenId = "";
    options.draftProvider.auth.apiKeyOptions = [];
    options.setApiKeyOptions([]);
    if (protocol === "api") {
      options.draftProvider.auth.mode = "apiKey";
    } else if (protocol === "sub2Api" && options.draftProvider.auth.mode === "session") {
      options.draftProvider.auth.mode = "password";
    }
    options.protocolSelectionSource.value = source;
    options.protocolSelectionBaseUrl.value = baseUrl;
    options.siteProbeResult.value = null;
    options.siteNameSourceBaseUrl.value = "";
    resetCredentialAssistant();
    return true;
  }

  function applySiteResult(result: ProviderSiteProbeResult, baseUrl: string) {
    options.siteProbeResult.value = result;
    if (result.systemName) {
      options.draftProvider.identity.name = result.systemName;
      options.siteNameSourceBaseUrl.value = normalizeProviderBaseUrl(baseUrl);
    }
  }

  function protocolLabel(protocol: ProviderProtocol) {
    if (protocol === "sub2Api") return "Sub2API";
    if (protocol === "api") return "通用 API Key";
    return "NewAPI";
  }

  return {
    probeSite,
    selectProtocol,
    ensureProtocolSelection,
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
