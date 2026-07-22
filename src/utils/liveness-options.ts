import type {
  LivenessIntervalMode,
  LivenessPromptMode,
  LivenessCliKind,
  TemporaryCliTerminalKind,
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
  { label: "Codex", value: "codex" },
  { label: "Claude Code", value: "claudeCode" },
];

export const temporaryCliTerminalOptions: SelectOption<TemporaryCliTerminalKind>[] = [
  { label: "自动检测", value: "auto" },
  { label: "系统默认", value: "systemDefault" },
  { label: "Terminal", value: "terminal" },
  { label: "iTerm2", value: "iTerm2" },
  { label: "Warp", value: "warp" },
  { label: "WezTerm", value: "wezTerm" },
  { label: "Kaku", value: "kaku" },
  { label: "Ghostty", value: "ghostty" },
  { label: "Kitty", value: "kitty" },
  { label: "Alacritty", value: "alacritty" },
  { label: "Windows Terminal", value: "windowsTerminal" },
  { label: "命令提示符", value: "commandPrompt" },
  { label: "PowerShell", value: "powerShell" },
  { label: "自定义命令", value: "custom" },
];

export function temporaryCliTerminalOptionsForPlatform(
  platform: string,
): SelectOption<TemporaryCliTerminalKind>[] {
  const common: SelectOption<TemporaryCliTerminalKind>[] = [
    { label: "自动检测", value: "auto" },
    { label: "系统默认", value: "systemDefault" },
  ];
  const custom: SelectOption<TemporaryCliTerminalKind>[] = [
    { label: "自定义命令", value: "custom" },
  ];

  if (platform === "windows") {
    return [
      ...common,
      { label: "Windows Terminal", value: "windowsTerminal" },
      { label: "命令提示符", value: "commandPrompt" },
      { label: "PowerShell", value: "powerShell" },
      ...custom,
    ];
  }

  if (platform === "linux") {
    return [
      ...common,
      { label: "Warp", value: "warp" },
      { label: "WezTerm", value: "wezTerm" },
      { label: "Ghostty", value: "ghostty" },
      { label: "Kitty", value: "kitty" },
      { label: "Alacritty", value: "alacritty" },
      ...custom,
    ];
  }

  return [
    ...common,
    { label: "Terminal", value: "terminal" },
    { label: "iTerm2", value: "iTerm2" },
    { label: "Warp", value: "warp" },
    { label: "WezTerm", value: "wezTerm" },
    { label: "Kaku", value: "kaku" },
    { label: "Ghostty", value: "ghostty" },
    { label: "Kitty", value: "kitty" },
    { label: "Alacritty", value: "alacritty" },
    ...custom,
  ];
}
