import type {
  LivenessIntervalMode,
  LivenessPromptMode,
} from "../stores/providers";

export interface SelectOption<T extends string = string> {
  label: string;
  value: T;
}

export const codexIntervalModeOptions: SelectOption<LivenessIntervalMode>[] = [
  { label: "固定周期", value: "fixed" },
  { label: "随机周期", value: "random" },
];

export const codexPromptModeOptions: SelectOption<LivenessPromptMode>[] = [
  { label: "固定话术", value: "fixed" },
  { label: "话术库随机", value: "random" },
  { label: "话术库轮询", value: "roundRobin" },
];
