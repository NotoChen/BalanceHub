import assert from "node:assert/strict";
import test from "node:test";

import type { Provider } from "../src/stores/provider-types.ts";
import {
  countProviderFilters,
  providerMatchesSearch,
  providerMatchesFilter,
} from "../src/utils/provider-filters.ts";
import { providerCardStatusTone } from "../src/utils/provider-display.ts";
import { useProviderCardTone } from "../src/composables/useProviderCardTone.ts";
import { ref } from "vue";

function provider(
  values: {
    name?: string;
    remark?: string;
    baseUrl?: string;
    username?: string;
    userId?: string;
    models?: string[];
    apiKey?: string;
    apiKeyRemarks?: string[];
    apiKeyRemoteNames?: string[];
    authMode?: Provider["auth"]["mode"];
    protocol?: Provider["identity"]["protocol"];
    enabled?: boolean;
    status?: Provider["runtime"]["status"];
    synced?: boolean;
    checkIn?: boolean;
    checkedIn?: boolean;
    available?: number;
    unlimited?: boolean;
    quotaKnown?: boolean;
  } = {},
) {
  return {
    identity: {
      id: "fixture-provider",
      protocol: values.protocol ?? "newApi",
      name: values.name ?? "Relay Site",
      remark: values.remark ?? "",
      displayName: "Relay",
      baseUrl: values.baseUrl ?? "https://relay.example.com/v1",
      backupUrls: [],
      username: values.username ?? "alice",
      userId: values.userId ?? "user-42",
    },
    auth: {
      mode: values.authMode ?? "password",
      apiUser: "",
      apiKey: values.apiKey ?? "sk-secret",
      apiKeyOptions: (values.apiKeyRemarks ?? []).map((localName, index) => ({
        localName,
        name: values.apiKeyRemoteNames?.[index] ?? "",
        key: index === 0 ? "key-local-secret" : "key-backup-secret",
      })),
    },
    cli: { preferredModel: "" },
    liveness: {
      model: "",
      agentBaseUrls: {},
      records: [],
    },
    capabilities: { availableModels: values.models ?? ["claude-sonnet-4"] },
    runtime: {
      enabled: values.enabled ?? true,
      status: values.status ?? "normal",
    },
    automation: {
      lastSyncedAt: values.synced === false ? null : "2026-09-27T07:00:00Z",
    },
    actions: {
      checkIn: values.checkIn ?? false,
      checkedInToday: values.checkedIn ?? false,
    },
    quota: {
      available: values.available ?? 100,
      used: 100,
      known: values.quotaKnown ?? true,
      totalKnown: true,
      unlimited: values.unlimited ?? false,
    },
  } as unknown as Provider;
}

test("provider search matches visible identity, endpoint, account and model fields", () => {
  const value = provider({
    name: "North Relay",
    remark: "Claude 主用",
    baseUrl: "https://gateway.example.net/v2",
    username: "xiaoming",
    userId: "uid-9088",
    models: ["gpt-5-codex"],
  });

  assert.equal(providerMatchesSearch(value, "north"), true);
  assert.equal(providerMatchesSearch(value, "claude 主用"), true);
  assert.equal(providerMatchesSearch(value, "gateway.example.net"), true);
  assert.equal(providerMatchesSearch(value, "xiaoming"), true);
  assert.equal(providerMatchesSearch(value, "uid-9088"), true);
  assert.equal(providerMatchesSearch(value, "GPT-5-CODEX"), true);
  assert.equal(providerMatchesSearch(value, "north uid-9088"), true);
  assert.equal(providerMatchesSearch(value, "missing"), false);
});

test("provider search does not inspect credentials", () => {
  const value = provider({ apiKey: "sk-only-for-authentication" });

  assert.equal(
    providerMatchesSearch(value, "sk-only-for-authentication"),
    false,
  );
  assert.equal(providerMatchesSearch(value, "   "), true);
});

test("provider search matches per-Key local remarks and remote names", () => {
  const value = provider({
    apiKeyRemarks: ["Codex 生产", "Claude 备用"],
    apiKeyRemoteNames: ["token-prod", "token-backup"],
  });

  assert.equal(providerMatchesSearch(value, "codex 生产"), true);
  assert.equal(providerMatchesSearch(value, "token-backup"), true);
  assert.equal(providerMatchesSearch(value, "claude token-backup"), true);
  assert.equal(providerMatchesSearch(value, "key-local-secret"), false);
});

test("status filters exclude disabled stations and distinguish unavailable, unlimited, and unknown balances", () => {
  const disabled = provider({
    enabled: false,
    status: "error",
    checkIn: true,
    available: 0,
  });
  const pending = provider({ status: "warning", synced: false });
  const failed = provider({ status: "error", checkIn: true });
  const due = provider({ checkIn: true });
  const empty = provider({ available: 0 });
  const unlimited = provider({ available: 0, unlimited: true });
  const unknown = provider({ available: 0, quotaKnown: false });
  const checked = provider({ checkIn: true, checkedIn: true });

  assert.equal(providerCardStatusTone(disabled), "disabled");
  assert.equal(providerMatchesFilter(disabled, "attention"), false);
  assert.equal(providerMatchesFilter(disabled, "checkIn"), false);
  assert.equal(providerMatchesFilter(pending, "attention"), true);
  assert.equal(providerMatchesFilter(failed, "attention"), true);
  assert.equal(providerMatchesFilter(failed, "checkIn"), false);
  assert.equal(providerMatchesFilter(empty, "attention"), true);
  assert.equal(providerMatchesFilter(unlimited, "attention"), false);
  assert.equal(providerMatchesFilter(unknown, "attention"), false);
  assert.deepEqual(
    countProviderFilters([
      disabled,
      pending,
      failed,
      due,
      empty,
      unlimited,
      unknown,
      checked,
    ]),
    {
      all: 8,
      checkIn: 1,
      attention: 3,
      newApi: 8,
      sub2Api: 0,
      api: 0,
    },
  );
});

test("attention includes exhausted balances even when the card prioritizes pending check-in", () => {
  const dueAndEmpty = provider({ checkIn: true, available: 0 });
  assert.equal(providerCardStatusTone(dueAndEmpty), "warning");
  assert.equal(providerMatchesFilter(dueAndEmpty, "checkIn"), true);
  assert.equal(providerMatchesFilter(dueAndEmpty, "attention"), true);
  assert.equal(
    providerMatchesFilter(
      provider({ checkIn: true, available: 0, unlimited: true }),
      "attention",
    ),
    false,
  );
  assert.equal(
    providerMatchesFilter(
      provider({ checkIn: true, available: 0, quotaKnown: false }),
      "attention",
    ),
    false,
  );
  assert.equal(
    providerMatchesFilter(
      provider({ checkIn: true, available: 0, enabled: false }),
      "attention",
    ),
    false,
  );
  assert.deepEqual(countProviderFilters([dueAndEmpty]), {
    all: 1,
    checkIn: 1,
    attention: 1,
    newApi: 1,
    sub2Api: 0,
    api: 0,
  });
});

test("protocol presets keep API Key credentials under their declared protocol and retain disabled stations", () => {
  const newAccount = provider({ protocol: "newApi" });
  const newKey = provider({ protocol: "newApi", authMode: "apiKey" });
  const subAccount = provider({ protocol: "sub2Api" });
  const subKey = provider({ protocol: "sub2Api", authMode: "apiKey" });
  const genericKey = provider({ protocol: "api", authMode: "apiKey" });
  const disabledKey = provider({
    protocol: "api",
    authMode: "apiKey",
    enabled: false,
  });
  const values = [
    newAccount,
    newKey,
    subAccount,
    subKey,
    genericKey,
    disabledKey,
  ];

  assert.deepEqual(
    values.filter((value) => providerMatchesFilter(value, "newApi")),
    [newAccount, newKey],
  );
  assert.deepEqual(
    values.filter((value) => providerMatchesFilter(value, "sub2Api")),
    [subAccount, subKey],
  );
  assert.deepEqual(
    values.filter((value) => providerMatchesFilter(value, "api")),
    [genericKey, disabledKey],
  );
  assert.deepEqual(
    values.filter((value) => providerMatchesFilter(value, "all")),
    values,
  );
  assert.deepEqual(countProviderFilters(values), {
    all: 6,
    checkIn: 0,
    attention: 0,
    newApi: 2,
    sub2Api: 2,
    api: 2,
  });
});

test("one preset combines with search while counts describe every available preset", () => {
  const dueAndEmpty = provider({
    name: "Production NewAPI",
    protocol: "newApi",
    checkIn: true,
    available: 0,
  });
  const generic = provider({
    name: "Production API",
    protocol: "api",
    authMode: "apiKey",
  });
  const development = provider({
    name: "Development Sub2API",
    protocol: "sub2Api",
    status: "error",
  });
  const scoped = [dueAndEmpty, generic, development].filter((value) =>
    providerMatchesSearch(value, "production"),
  );

  assert.deepEqual(countProviderFilters(scoped), {
    all: 2,
    checkIn: 1,
    attention: 1,
    newApi: 1,
    sub2Api: 0,
    api: 1,
  });
  for (const filter of ["newApi", "checkIn", "attention"] as const) {
    assert.deepEqual(
      scoped.filter((value) => providerMatchesFilter(value, filter)),
      [dueAndEmpty],
    );
  }
  assert.deepEqual(
    scoped.filter((value) => providerMatchesFilter(value, "api")),
    [generic],
  );
  assert.deepEqual(
    scoped.filter((value) => providerMatchesFilter(value, "sub2Api")),
    [],
  );
  assert.deepEqual(
    scoped.filter((value) => providerMatchesFilter(value, "all")),
    [dueAndEmpty, generic],
  );
});

test("pending check-in stays in its filter while the card shows progress and disappears only after completion", () => {
  const providers = ref([provider({ checkIn: true })]);
  const checking = ref([providers.value[0].identity.id]);
  const tones = useProviderCardTone({
    providers,
    checkingInProviderIds: checking,
    probingCapabilitiesProviderId: ref(null),
    editingProviderId: ref(null),
    probingSite: ref(false),
    testingConnection: ref(false),
    completingCredentials: ref(false),
  });
  assert.equal(tones.providerCardTone(providers.value[0]), "syncing");
  assert.equal(providerMatchesFilter(providers.value[0], "checkIn"), true);
  checking.value = [];
  assert.equal(tones.providerCardTone(providers.value[0]), "warning");
  providers.value[0].actions.checkedInToday = true;
  assert.equal(tones.providerCardTone(providers.value[0]), "ok");
  assert.equal(providerMatchesFilter(providers.value[0], "checkIn"), false);
  providers.value[0].runtime.enabled = false;
  checking.value = [providers.value[0].identity.id];
  assert.equal(tones.providerCardTone(providers.value[0]), "disabled");
});
