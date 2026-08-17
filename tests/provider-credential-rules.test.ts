import assert from "node:assert/strict";
import test from "node:test";

import { canRunCredentialAssistantForInput } from "../src/composables/provider-credential-rules.ts";
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
    authModes: [
      {
        mode: "password",
        label: "账号密码",
        description: "",
        note: "",
        requiredFields: ["loginUsername", "loginPassword"],
        optionalFields: [],
        fields: [],
      },
      {
        mode: "accessToken",
        label: "访问令牌",
        description: "",
        note: "",
        requiredFields: ["accessToken", "apiUser"],
        optionalFields: [],
        fields: [],
      },
      {
        mode: "apiKey",
        label: "API Key",
        description: "",
        note: "",
        requiredFields: ["apiKey"],
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
      apiKeyRequiredFields: ["apiUser"],
      apiKeyRequiredAnyFields: ["sessionCookie", "accessToken"],
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
    },
    cli: { preferredModel: "" },
    automation: { refreshInterval: 0, checkInTime: "" },
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
