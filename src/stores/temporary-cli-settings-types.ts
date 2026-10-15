import type {
  AgentModelCatalogContext,
  AgentNativeSetting,
  AgentSettingChoice,
  AgentSettingReference,
  AgentSettingValueSummary,
} from "./agent-settings-types";
import type { AgentCliKind, AgentAssetCategory } from "./provider-types";

export interface TemporaryCliRunSettings {
  keepSession: boolean;
  nativeOptions: Record<string, string>;
  loadRules: boolean;
  initialPrompt: string;
  instructions: string;
  additionalDirectories: string[];
  disabledAssets: string[];
  disableSkills: boolean;
  disableMcp: boolean;
  disablePlugins: boolean;
  disableHooks: boolean;
}

export interface TemporaryCliProfile {
  id: string;
  name: string;
  cliKind: AgentCliKind;
  settings: TemporaryCliRunSettings;
}

export interface TemporaryCliAssetControl {
  key: "disableSkills" | "disableMcp" | "disablePlugins" | "disableHooks";
  label: string;
  nativeKey: string;
  description: string;
}

export interface TemporaryCliSettingsCapabilities {
  canDiscardSession: boolean;
  retentionNote: string;
  nativeSettings: AgentNativeSetting[];
  model: AgentSettingValueSummary;
  defaultModel: AgentSettingValueSummary;
  modelNativeKey: string;
  modelChoices: AgentSettingChoice[];
  modelCatalog: AgentModelCatalogContext;
  notices: string[];
  assetControls: TemporaryCliAssetControl[];
  runReferences: Record<string, AgentSettingReference>;
  canSkipRules: boolean;
  canAddDirectories: boolean;
  canDisableSkills: boolean;
  canDisableMcp: boolean;
  canDisablePlugins: boolean;
  canDisableHooks: boolean;
}

export interface TemporaryCliAssetOption {
  id: string;
  label: string;
  category: AgentAssetCategory;
  configurable: boolean;
  note: string;
  currentLabel: string;
}

export interface TemporaryCliAssetListing {
  assets: TemporaryCliAssetOption[];
  notice: string;
}

export interface TemporaryCliSettingsRequest {
  nativeOptions?: Record<string, string>;
  inheritedModel?: string;
  cliKind: AgentCliKind;
  cliPath: string;
  workdir: string;
  model: string;
  sessionMode: "new" | "history";
}

export function defaultTemporaryCliRunSettings(): TemporaryCliRunSettings {
  return {
    keepSession: true,
    nativeOptions: {},
    loadRules: true,
    initialPrompt: "",
    instructions: "",
    additionalDirectories: [],
    disabledAssets: [],
    disableSkills: false,
    disableMcp: false,
    disablePlugins: false,
    disableHooks: false,
  };
}

export function copyTemporaryCliRunSettings(
  value: TemporaryCliRunSettings,
): TemporaryCliRunSettings {
  return {
    ...value,
    nativeOptions: { ...value.nativeOptions },
    additionalDirectories: [...value.additionalDirectories],
    disabledAssets: [...value.disabledAssets],
  };
}

export function sameTemporaryCliRunSettings(
  left: TemporaryCliRunSettings,
  right: TemporaryCliRunSettings,
): boolean {
  const canonical = (value: TemporaryCliRunSettings) =>
    JSON.stringify({
      ...defaultTemporaryCliRunSettings(),
      ...value,
      nativeOptions: Object.fromEntries(
        Object.entries(value.nativeOptions).sort(([a], [b]) =>
          a.localeCompare(b),
        ),
      ),
      disabledAssets: [...value.disabledAssets].sort(),
    });
  return canonical(left) === canonical(right);
}
