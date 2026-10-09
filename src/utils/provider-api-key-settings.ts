import type {
  ProviderApiKeyEditorContext,
  ProviderApiKeyPatch,
  ProviderApiKeySettings,
} from "../stores/provider-types.ts";

export function apiKeyRestrictionLines(text: string) {
  return [
    ...new Set(
      text
        .split(/[,\n\r]+/)
        .map((item) => item.trim())
        .filter(Boolean),
    ),
  ];
}

/** Project form edits into the backend's patch contract; untouched fields stay omitted. */
export function apiKeySettingsPatch(
  context: ProviderApiKeyEditorContext,
  draft: ProviderApiKeySettings,
  creating: boolean,
  customKey = "",
): ProviderApiKeyPatch {
  const fields: (keyof ProviderApiKeySettings)[] = [
    "name",
    "group",
    "quota",
    "expiration",
    "allowIps",
  ];
  if (!creating) fields.push("enabled");
  if (context.supportsIpBlacklist) fields.push("denyIps");
  if (context.supportsModelLimits)
    fields.push("modelLimitsEnabled", "modelLimits");
  if (context.supportsSpendingLimits) fields.push("spendingLimits");
  if (draft.group === context.automaticGroup) {
    if (context.supportsCrossGroupRetry) fields.push("crossGroupRetry");
    if (context.autoGroups) fields.push("autoGroups");
  }
  const normalized = { ...draft, name: draft.name.trim() };
  const patch = Object.fromEntries(
    fields
      .filter(
        (field) =>
          creating ||
          JSON.stringify(normalized[field]) !==
            JSON.stringify(context.settings[field]),
      )
      .map((field) => [field, normalized[field]]),
  ) as ProviderApiKeyPatch;
  if (creating && context.supportsCustomKey && customKey.trim())
    patch.customKey = customKey.trim();
  return patch;
}
