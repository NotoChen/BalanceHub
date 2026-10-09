import * as vue from "vue";
import { effectScope, nextTick } from "vue";
import type { TestContext } from "node:test";
import * as inputHelpers from "../../src/utils/provider-input.ts";
import * as apiKeyOptions from "../../src/utils/provider-api-key-options.ts";
import * as shared from "../../src/composables/provider-editor-shared.ts";
import * as rules from "../../src/composables/provider-credential-rules.ts";
import * as protocolHelpers from "../../src/utils/provider-protocol.ts";
import * as display from "../../src/utils/provider-display.ts";
import { withTimeout } from "../../src/utils/promise-timeout.ts";
import { useProviderSave } from "../../src/composables/useProviderSave.ts";
import { useProviderConnectionTest } from "../../src/composables/useProviderConnectionTest.ts";
import type { ProviderEditorStore } from "../../src/composables/provider-editor-shared.ts";
import type { AuthMode, Provider, ProviderApiKeyOption, ProviderInput, ProviderProtocol, ProviderProtocolDescriptor } from "../../src/stores/provider-types.ts";
import { loadSource } from "./load-source.ts";

export function protocolFixture(kind: ProviderProtocol): ProviderProtocolDescriptor {
  const modes: AuthMode[] = kind === "newApi" ? ["password", "session", "accessToken", "apiKey"] : kind === "sub2Api" ? ["password", "accessToken", "apiKey"] : ["apiKey"];
  return {
    kind, label: kind, description: "", defaultAuthMode: kind === "api" ? "apiKey" : "password", browserLoginSupported: kind === "newApi",
    authModes: modes.map((mode) => ({
      mode, label: mode, description: "", note: "", optionalFields: [], fields: [],
      requiredFields: mode === "password" ? ["loginUsername", "loginPassword"] : mode === "accessToken" ? (kind === "newApi" ? ["accessToken", "apiUser"] : ["accessToken"]) : mode === "apiKey" ? ["apiKey"] : [],
      requiredAnyFields: mode === "session" ? ["sessionCookie", "newApiSession.refreshCookie"] : [],
    })),
    capabilities: { accessToken: kind !== "api", apiKeyManagement: kind !== "api", usage: true, account: kind !== "api", checkIn: kind === "newApi", announcements: false },
    operationMethods: { checkIn: null, apiKeys: null, invitation: null, models: "", announcements: null },
    credentialAssistant: { enabled: kind !== "api", accessTokenFlow: kind === "newApi" ? "sessionGeneration" : kind === "sub2Api" ? "credentialCompletion" : "none", accessTokenSkipFields: kind === "newApi" ? ["newApiSession.refreshCookie"] : [], apiKeyRequiredFields: kind === "newApi" ? ["apiUser"] : [], apiKeyRequiredAnyFields: kind === "newApi" ? ["sessionCookie", "accessToken", "newApiSession.refreshCookie"] : [] },
  };
}

export const keyFixture = (key = "sk-fixture-key", tokenId = "fixture-key-1") => ({
  ...apiKeyOptions.effectiveProviderApiKeyOptions(key, [])[0], tokenId,
});

export function completedCredentials(input: ProviderInput, keys = [keyFixture()]) {
  const updated = structuredClone(input);
  if (input.identity.protocol === "newApi") {
    updated.auth.apiUser = "42";
    updated.auth.newApiSession = { refreshCookie: "fixture-refresh-cookie", sessionId: "fixture-session", accessToken: "fixture-session-jwt", accessExpiresAt: 4102444800 };
  } else {
    updated.auth.accessToken = "fixture-access-token";
    updated.auth.refreshToken = "fixture-refresh-token";
  }
  updated.auth.apiKeyOptions = keys;
  if (keys.length === 1) {
    updated.auth.apiKey = keys[0].key;
    updated.auth.apiKeyTokenId = keys[0].tokenId;
  }
  return { input: updated, changedFields: ["apiKeyOptions"], steps: keys.length ? [] : [{ name: "读取 API Key", ok: false, message: "站点没有已有 API Key" }], apiKeyOptions: keys };
}

export function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (failure: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

export async function settle() {
  for (let index = 0; index < 24; index++) await Promise.resolve();
  await nextTick();
}

export function providerEditorFixture(t: TestContext, overrides: {
  complete?: ProviderEditorStore["completeProviderCredentials"];
  detect?: ProviderEditorStore["detectProviderProtocol"];
  generate?: ProviderEditorStore["generateAccessTokenForInput"];
  chooseKey?: (keys: ProviderApiKeyOption[], signal: AbortSignal) => Promise<ProviderApiKeyOption | null>;
  confirm?: (...args: unknown[]) => Promise<boolean>;
  openKeyEditor?: (...args: unknown[]) => { result: Promise<ProviderApiKeyOption | null>; close: () => void };
} = {}) {
  const feedback: string[] = [];
  const Message = Object.fromEntries(["success", "warning", "error", "info"].map((kind) => [kind, (message: string) => feedback.push(message)]));
  const dialogs = { confirmAction: overrides.confirm ?? (async () => false), chooseProviderApiKey: overrides.chooseKey ?? (async () => null), chooseSameSiteApiKeyAction: async () => "cancel" };
  const assistant = loadSource<typeof import("../../src/composables/useProviderCredentialAssistant.ts")>("composables/useProviderCredentialAssistant.ts", {
    "@arco-design/web-vue": { Message }, "./provider-credential-dialogs": dialogs,
    "./provider-api-key-editor": { openApiKeyEditor: overrides.openKeyEditor ?? (() => ({ result: Promise.resolve(null), close() {} })) },
    "../utils/promise-timeout": { withTimeout }, "./provider-credential-rules": rules,
    "./provider-editor-shared": shared, "../utils/provider-protocol": protocolHelpers, "../utils/provider-display": display,
  });
  const completion = loadSource<typeof import("../../src/composables/useProviderCredentialCompletion.ts")>("composables/useProviderCredentialCompletion.ts", {
    "@arco-design/web-vue": { Message }, "../utils/promise-timeout": { withTimeout }, "./provider-editor-shared": shared,
    "../utils/provider-protocol": protocolHelpers, "./provider-credential-dialogs": dialogs, "./useProviderCredentialAssistant": assistant,
  });
  const state = loadSource<typeof import("../../src/composables/useProviderEditorState.ts")>("composables/useProviderEditorState.ts", {
    "../utils/provider-input": inputHelpers, "../utils/provider-api-key-options": apiKeyOptions, "./provider-editor-shared": shared,
  });
  const factory = loadSource<typeof import("../../src/composables/useProviderEditor.ts")>("composables/useProviderEditor.ts", {
    vue: { ...vue, onUnmounted: vue.onScopeDispose },
    "@arco-design/web-vue": { Message }, "./useClipboard": { copyText: async () => {} }, "./provider-editor-shared": shared,
    "./useProviderConnectionTest": { useProviderConnectionTest }, "./useProviderCredentialCompletion": completion,
    "./useProviderEditorState": state, "./useProviderSave": { useProviderSave }, "./provider-credential-dialogs": dialogs,
    "../utils/provider-display": display, "../utils/liveness-defaults": { normalizeLivenessTiming() {} },
    "../api/provider-browser-login": { startProviderBrowserLogin() {}, cancelProviderBrowserLogin() {} },
    "../utils/provider-login-flow": { createProviderLoginFlow: () => ({ run() {}, invalidate() {} }) },
  });
  const writes: ProviderInput[] = [];
  let completionCalls = 0;
  let generationCalls = 0;
  let protocolProbeCalls = 0;
  const store = {
    providerProtocols: [protocolFixture("newApi"), protocolFixture("sub2Api"), protocolFixture("api")],
    detectProviderProtocol: async (input: ProviderInput) => {
      protocolProbeCalls++;
      if (!overrides.detect) throw new Error("Unexpected protocol probe");
      return overrides.detect(input);
    },
    completeProviderCredentials: async (input: ProviderInput) => { completionCalls++; return (overrides.complete ?? completedCredentials)(input); },
    saveProvider: async (input: ProviderInput) => {
      writes.push(structuredClone(input));
      return { saved: true, conflict: null, provider: { ...input, identity: { ...input.identity, id: input.id || "fixture-provider" } } as Provider };
    },
    generateAccessTokenForInput: async (input: ProviderInput) => { generationCalls++; return overrides.generate ? overrides.generate(input) : "fixture-generated-token"; },
  } as unknown as ProviderEditorStore;
  const scope = effectScope();
  const editor = scope.run(() => factory.useProviderEditor({ store, browserRuntime: {} as never, loginAccounts: {} as never }))!;
  t.after(() => scope.stop());
  editor.openAddProvider();
  function configure(protocol: ProviderProtocol = "newApi") {
    const draft = editor.draftProvider;
    draft.identity.baseUrl = "https://fixture.invalid";
    draft.identity.protocol = protocol;
    draft.auth.mode = protocol === "api" ? "apiKey" : "password";
    draft.auth.loginUsername = "fixture-user";
    draft.auth.loginPassword = "fixture-password";
    draft.runtime.enabled = false;
    editor.protocolSelectionSource.value = "manual";
    editor.protocolSelectionBaseUrl.value = draft.identity.baseUrl;
  }
  configure();
  return { editor, writes, feedback, configure, scope, completionCalls: () => completionCalls, generationCalls: () => generationCalls, protocolProbeCalls: () => protocolProbeCalls };
}
