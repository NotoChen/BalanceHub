import type {
  AgentNativeSetting,
  AgentSettingValueSummary,
} from "../stores/agent-settings-types";

export function currentSettingValue(
  field: AgentNativeSetting,
): AgentSettingValueSummary {
  return {
    value: field.currentValue,
    label: field.currentLabel,
    source: field.source,
    state: field.currentState,
    detail: field.currentDetail,
  };
}

export function settingValueLabel(
  value: AgentSettingValueSummary,
  field?: AgentNativeSetting,
): string {
  if (value.value !== null && value.label === value.value && field) {
    const choice = field.choices.find((choice) => choice.value === value.value);
    if (choice) return choice.label;
    if (
      field.control === "number" &&
      value.value.trim() &&
      Number.isFinite(Number(value.value))
    )
      return `${Number(value.value).toLocaleString("zh-CN")} ${field.unit}`.trim();
  }
  return value.label;
}

export function settingValueBadge(value: AgentSettingValueSummary): string {
  return {
    configured: "已配置",
    default: "默认值",
    unset: "未设置",
    readError: "读取失败",
  }[value.state];
}

export function settingValueDescription(
  value: AgentSettingValueSummary,
): string {
  return [value.source, value.detail].filter(Boolean).join("\n");
}

export function knownSettingValue(
  value: AgentSettingValueSummary,
): string | null {
  return value.state === "configured" || value.state === "default"
    ? value.value
    : null;
}

export function settingReadFailed(field: AgentNativeSetting): boolean {
  return (
    field.currentState === "readError" ||
    Boolean(
      field.modelMappings?.entries.some(
        (entry) => entry.current.state === "readError",
      ),
    )
  );
}
