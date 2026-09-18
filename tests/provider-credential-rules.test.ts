import assert from "node:assert/strict";
import test from "node:test";

import {
  canRunCredentialAssistantForInput,
  canSkipAssistantAccessToken,
  credentialFieldHasValue,
  missingCredentialRequirements,
} from "../src/composables/provider-credential-rules.ts";
import type {
  ProviderInput,
  ProviderProtocolDescriptor,
} from "../src/stores/provider-types.ts";

const descriptors: ProviderProtocolDescriptor[] = [
  {
    kind: "newApi",
    label: "NewAPI",
    description: "",
    defaultAuthMode: "password",
    browserLoginSupported: true,
    authModes: [
      {
        mode: "password",
        label: "账号密码",
        description: "",
        note: "",
        requiredFields: ["loginUsername", "loginPassword"],
        requiredAnyFields: [],
        optionalFields: [],
        fields: [],
      },
      {
        mode: "session",
        label: "登录会话",
        description: "",
        note: "",
        requiredFields: [],
        requiredAnyFields: ["sessionCookie", "newApiSession.refreshCookie"],
        optionalFields: [],
        fields: [],
      },
      {
        mode: "accessToken",
        label: "访问令牌",
        description: "",
        note: "",
        requiredFields: ["accessToken", "apiUser"],
        requiredAnyFields: [],
        optionalFields: [],
        fields: [],
      },
      {
        mode: "apiKey",
        label: "API Key",
        description: "",
        note: "",
        requiredFields: ["apiKey"],
        requiredAnyFields: [],
        optionalFields: [],
        fields: [],
      },
    ],
    capabilities: {
      accessToken: true,
      apiKeyManagement: true,
      usage: true,
      account: true,
      checkIn: true,
    },
    operationMethods: {
      checkIn: "",
      apiKeys: "",
      invitation: "",
      models: "",
    },
    credentialAssistant: {
      enabled: true,
      accessTokenFlow: "sessionGeneration",
      accessTokenSkipFields: ["newApiSession.refreshCookie"],
      apiKeyRequiredFields: ["apiUser"],
      apiKeyRequiredAnyFields: ["sessionCookie", "accessToken", "newApiSession.refreshCookie"],
    },
  },
];

function input(): ProviderInput {
  return {
    identity: {
      name: "",
      baseUrl: "",
      protocol: "newApi",
      userId: "",
      backupUrls: [],
    },
    auth: {
      mode: "password",
      apiKey: "",
      apiKeyTokenId: "",
      apiKeyOptions: [],
      accessToken: "",
      sessionCookie: "",
      apiUser: "",
      loginUsername: "",
      loginPassword: "",
      refreshToken: "",
      accessTokenExpiresAt: null,
      newApiSession: null,
    },
    cli: { preferredModel: "" },
    automation: { refreshInterval: 0, checkInTime: "", checkInMethod: "auto", autoShield: true, turnstileMode: "auto" },
    liveness: {
      useGlobal: true,
      enabled: false,
      agentBaseUrls: {},
      cliKind: null,
      intervalMode: "fixed",
      interval: 300,
      randomMinInterval: 300,
      randomMaxInterval: 300,
      timeout: 75,
      model: "",
      promptMode: "random",
      fixedPrompt: "",
    },
    proxy: { mode: "inherit", url: "" },
    notification: { mode: "inherit", channelIds: [] },
    runtime: { enabled: true },
  };
}

test("credential assistant follows the Rust-provided required field schema", () => {
  const draft = input();
  draft.identity.baseUrl = "https://relay.example.com";
  draft.auth.loginUsername = "alice";

  assert.equal(canRunCredentialAssistantForInput(draft, descriptors, false), false);

  draft.auth.loginPassword = "password";
  assert.equal(canRunCredentialAssistantForInput(draft, descriptors, false), true);
});

test("credential assistant does not duplicate access-token requirements in TypeScript", () => {
  const draft = input();
  draft.identity.baseUrl = "https://relay.example.com";
  draft.auth.mode = "accessToken";
  draft.auth.accessToken = "token";

  assert.equal(canRunCredentialAssistantForInput(draft, descriptors, false), false);

  draft.auth.apiUser = "42";
  assert.equal(canRunCredentialAssistantForInput(draft, descriptors, false), true);
});

test("API Key mode and busy state remain outside the credential assistant workflow", () => {
  const draft = input();
  draft.identity.baseUrl = "https://relay.example.com";
  draft.auth.mode = "apiKey";
  draft.auth.apiKey = "sk-key";

  assert.equal(canRunCredentialAssistantForInput(draft, descriptors, false), false);

  draft.auth.mode = "password";
  draft.auth.loginUsername = "alice";
  draft.auth.loginPassword = "password";
  assert.equal(canRunCredentialAssistantForInput(draft, descriptors, true), false);
});

test("credential assistant availability is owned by the protocol descriptor", () => {
  const draft = input();
  draft.identity.baseUrl = "https://relay.example.com";
  draft.auth.loginUsername = "alice";
  draft.auth.loginPassword = "password";
  const disabled = [{
    ...descriptors[0],
    credentialAssistant: {
      ...descriptors[0].credentialAssistant,
      enabled: false,
    },
  }];

  assert.equal(canRunCredentialAssistantForInput(draft, disabled, false), false);
});

test("unknown schema fields fail closed instead of being treated as completed", () => {
  const draft = input();
  draft.identity.baseUrl = "https://relay.example.com";
  draft.auth.loginUsername = "alice";
  draft.auth.loginPassword = "password";
  const extended = [{
    ...descriptors[0],
    authModes: descriptors[0].authModes.map((mode) => mode.mode === "password"
      ? { ...mode, requiredFields: [...mode.requiredFields, "futureCredential"] }
      : mode),
  }];

  assert.equal(canRunCredentialAssistantForInput(draft, extended, false), false);
});

test("imported renewable sessions enable the assistant without a classic cookie or PAT", () => {
  const draft = input();
  draft.identity.baseUrl = "https://relay.example.com";
  draft.auth.mode = "session";
  draft.auth.apiUser = "42";
  draft.auth.newApiSession = {
    refreshCookie: "fixture-refresh",
    sessionId: "fixture-session",
    accessToken: "",
    accessExpiresAt: null,
  };
  const schema = descriptors[0].authModes.find((mode) => mode.mode === "session")!;

  assert.equal(canRunCredentialAssistantForInput(draft, descriptors, false), true);
  assert.deepEqual(missingCredentialRequirements(draft, schema), []);
  assert.equal(canSkipAssistantAccessToken(draft, descriptors[0]), true);
  assert.equal(descriptors[0].credentialAssistant.apiKeyRequiredAnyFields
    .some((field) => credentialFieldHasValue(draft, field)), true);

  draft.auth.newApiSession.refreshCookie = " ";
  assert.equal(canRunCredentialAssistantForInput(draft, descriptors, false), false);
  assert.equal(canSkipAssistantAccessToken(draft, descriptors[0]), false);
  assert.deepEqual(missingCredentialRequirements(draft, schema), [schema.requiredAnyFields]);

  draft.auth.newApiSession = null;
  draft.auth.sessionCookie = "session=fixture-classic";
  assert.equal(canRunCredentialAssistantForInput(draft, descriptors, false), true);
  assert.equal(canSkipAssistantAccessToken(draft, descriptors[0]), false);
});

test("modern sessions do not bypass a different selected mode or backend-declared requirements", () => {
  const draft = input();
  draft.identity.baseUrl = "https://relay.example.com";
  draft.auth.newApiSession = {
    refreshCookie: "fixture-refresh",
    sessionId: "fixture-session",
    accessToken: "fixture-jwt",
    accessExpiresAt: 900,
  };
  assert.equal(canRunCredentialAssistantForInput(draft, descriptors, false), false);

  draft.auth.mode = "accessToken";
  draft.auth.apiUser = "42";
  assert.equal(canRunCredentialAssistantForInput(draft, descriptors, false), false);

  draft.auth.mode = "session";
  const classicOnly = [{
    ...descriptors[0],
    authModes: descriptors[0].authModes.map((mode) => mode.mode === "session"
      ? { ...mode, requiredAnyFields: ["sessionCookie"] }
      : mode),
    credentialAssistant: { ...descriptors[0].credentialAssistant, accessTokenSkipFields: [] },
  }];
  assert.equal(canRunCredentialAssistantForInput(draft, classicOnly, false), false);
  assert.equal(canSkipAssistantAccessToken(draft, classicOnly[0]), false);
  assert.equal(credentialFieldHasValue(draft, "newApiSession.unknown"), false);
  assert.equal(credentialFieldHasValue(draft, "newApiSession.__proto__"), false);
});
