import type { AgentAssetScope, AgentCliKind } from "./provider-types";

export interface AgentSettingChoice {
  value: string;
  label: string;
  description: string;
}

export interface AgentModelCatalogContext {
  cliKind: AgentCliKind;
  cliPath: string;
  configRoot: string;
  catalogPath: string | null;
}

export interface AgentModelCatalog {
  choices: AgentSettingChoice[];
  source: string;
  notice: string;
  stale: boolean;
  refreshedAt: number | null;
}

export type AgentSettingValueState =
  "configured" | "default" | "unset" | "readError";

export interface AgentSettingValueSummary {
  value: string | null;
  label: string;
  source: string;
  state: AgentSettingValueState;
  detail: string;
}

export interface AgentNativeSetting {
  id: string;
  section: "model" | "permissions" | "tools" | "context";
  label: string;
  nativeKey: string;
  sourceReferences: AgentSettingSourceReference[];
  description: string;
  control:
    | "model"
    | "modelMappings"
    | "select"
    | "number"
    | "multiSelect"
    | "rules"
    | "readonly";
  hint: string;
  currentValue: string | null;
  currentNativeValue: string | null;
  currentLabel: string;
  currentState: AgentSettingValueState;
  currentDetail: string;
  resetValue: AgentSettingEffectiveValue | null;
  source: string;
  choices: AgentSettingChoice[];
  modelMappings?: AgentModelMappingOptions | null;
  allowCustom: boolean;
  minimum: number | null;
  maximum: number | null;
  sliderMaximum: number | null;
  percentageBase: number | null;
  minimumItems: number;
  unit: string;
  lockedReason: string;
  whenOption: [string, string] | null;
  exclusiveGroup: string | null;
}

export interface AgentSettingSourceReference {
  key: string;
  rowId: string | null;
  from: number;
  to: number;
  valueEnd: number;
}

export interface AgentModelMappingValue {
  model?: string;
  displayName?: string;
  extends?: string;
}

export interface AgentModelMappingEntry {
  id: string;
  label: string;
  nativeKey: string;
  current: AgentSettingValueSummary;
  targetDefault: AgentSettingValueSummary | null;
  resetValue: AgentSettingValueSummary | null;
  lockedReason: string;
  supportsDisplayName: boolean;
}

export interface AgentModelMappingOptions {
  allowCustomAliases: boolean;
  displayNames: boolean;
  inheritance: boolean;
  entries: AgentModelMappingEntry[];
}

export interface AgentSettingReference {
  nativeKey: string;
  description: string;
}

export interface AgentSettingEffectiveValue extends AgentSettingValueSummary {
  modelMappings: AgentModelMappingOptions | null;
}
export interface AgentConfigurationForm {
  sourceId: string;
  scope: AgentAssetScope;
  profile: string | null;
  text: string;
  fields: AgentNativeSetting[];
  modelCatalog: AgentModelCatalogContext;
  values: Record<string, string>;
  effective: Record<string, AgentSettingEffectiveValue>;
  inherited: Record<string, AgentSettingEffectiveValue>;
  notices: string[];
}
export interface AgentConfigurationFormRequest {
  editId: string;
  sourceId: string;
  expectedRevision: string;
  text: string;
  changes: Record<string, string | null>;
}
