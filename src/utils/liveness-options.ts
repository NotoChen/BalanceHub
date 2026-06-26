import type {
  LivenessIntervalMode,
  LivenessPromptMode,
  LivenessCliKind,
  LivenessHttpProtocol,
  LivenessMethod,
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

export const livenessCliKindOptions: SelectOption<LivenessCliKind>[] = [
  { label: "Codex CLI", value: "codex" },
  { label: "Claude Code CLI", value: "claudeCode" },
];

export const livenessMethodOptions: SelectOption<LivenessMethod>[] = [
  { label: "本地 CLI", value: "cli" },
  { label: "HTTP 调用", value: "http" },
];

export const livenessHttpProtocolOptions: SelectOption<LivenessHttpProtocol>[] = [
  { label: "OpenAI Chat Completions", value: "openaiChat" },
  { label: "OpenAI Responses", value: "openaiResponses" },
  { label: "Anthropic Messages", value: "anthropic" },
];
