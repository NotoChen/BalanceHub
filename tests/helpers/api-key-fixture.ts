import type { ProviderApiKeyEditorContext } from "../../src/stores/provider-types.ts";

export function keyEditorContext(): ProviderApiKeyEditorContext {
  return {
    credentialRevision: 4,
    settings: {
      name: "fixture",
      group: "group-a",
      quota: { unlimited: false, amount: 10 },
      expiration: { mode: "never" },
      allowIps: ["10.0.0.0/8"],
      denyIps: [],
      modelLimits: ["model-a"],
      modelLimitsEnabled: true,
      crossGroupRetry: false,
      autoGroups: [],
      spendingLimits: { fiveHours: 0, oneDay: 0, sevenDays: 0 },
      enabled: true,
    },
    groups: [
      { value: "group-a", label: "A", description: "", rate: 1 },
      { value: "group-b", label: "B", description: "", rate: 2 },
    ],
    groupClearable: true,
    defaultGroupLabel: "跟随账号分组",
    quotaLabel: "剩余额度",
    quotaUnit: "$",
    quotaMinimum: 0,
    expirationInDays: false,
    supportsIpBlacklist: false,
    supportsModelLimits: true,
    supportsCrossGroupRetry: true,
    automaticGroup: "auto",
    supportsSpendingLimits: false,
    supportsCustomKey: false,
    autoGroups: null,
    modelOptions: ["model-a"],
    modelOptionsError: null,
  };
}
