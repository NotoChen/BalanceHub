import type {
  LivenessIntervalMode,
  LivenessPromptMode,
} from "../stores/providers";

export interface SelectOption<T extends string = string> {
  label: string;
  value: T;
}

export const livenessIntervalModeOptions: SelectOption<LivenessIntervalMode>[] = [
  { label: "固定周期", value: "fixed" },
  { label: "随机周期", value: "random" },
];

export const livenessPromptModeOptions: SelectOption<LivenessPromptMode>[] = [
  { label: "固定提示词", value: "fixed" },
  { label: "随机抽取模板", value: "random" },
  { label: "依次使用模板", value: "roundRobin" },
];
