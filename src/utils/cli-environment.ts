import type {
  AppSettings,
  CliEnvironmentProbeResult,
  LivenessCliKind,
  TemporaryCliSessionMode,
  TemporaryCliTerminalKind,
  TerminalEnvironmentProbeResult,
  TemporaryTerminalProbeResult,
} from "../stores/providers";
import type { SelectOption } from "./liveness-options";

export const cliKindMeta: Record<
  LivenessCliKind,
  { label: string; brand: "codex" | "claude" }
> = {
  codex: { label: "Codex", brand: "codex" },
  claudeCode: { label: "Claude Code", brand: "claude" },
};

export function canNameSessionAtLaunch(
  probe: CliEnvironmentProbeResult | null | undefined,
  cliKind: LivenessCliKind,
  sessionMode: TemporaryCliSessionMode,
) {
  if (sessionMode !== "new" || !probe) return false;
  const tool = cliKind === "codex" ? probe.codex : probe.claudeCode;
  return tool.available && tool.supportsSessionName;
}

export function availableCliKinds(
  probe: CliEnvironmentProbeResult | null | undefined,
): LivenessCliKind[] {
  if (!probe) return [];
  const kinds: LivenessCliKind[] = [];
  if (probe.codex.available) kinds.push("codex");
  if (probe.claudeCode.available) kinds.push("claudeCode");
  return kinds;
}

export function availableCliOptions(
  probe: CliEnvironmentProbeResult | null | undefined,
): SelectOption<LivenessCliKind>[] {
  return availableCliKinds(probe).map((value) => ({
    value,
    label: cliKindMeta[value].label,
  }));
}

export function availableTerminalResults(
  probe: TerminalEnvironmentProbeResult | null | undefined,
): TemporaryTerminalProbeResult[] {
  if (!probe) return [];
  const seen = new Set<TemporaryCliTerminalKind>();
  return probe.terminals.filter((terminal) => {
    if (!terminal.available || seen.has(terminal.kind)) return false;
    seen.add(terminal.kind);
    return true;
  });
}

export function availableTerminalOptions(
  probe: TerminalEnvironmentProbeResult | null | undefined,
): SelectOption<TemporaryCliTerminalKind>[] {
  return availableTerminalResults(probe).map((terminal) => ({
    value: terminal.kind,
    label: terminal.name,
  }));
}

export type TerminalEnvironmentSettingsSnapshot = Pick<
  AppSettings,
  "temporaryCliTerminalKind"
>;

export function captureTerminalEnvironmentSettings(
  settings: AppSettings,
): TerminalEnvironmentSettingsSnapshot {
  return {
    temporaryCliTerminalKind: settings.temporaryCliTerminalKind,
  };
}

/** 只在用户明确触发设置页扫描时修正已失效的终端选择，并保留扫描期间的用户修改。 */
export function applyTerminalEnvironmentProbeResult(
  settings: AppSettings,
  probe: TerminalEnvironmentProbeResult,
  expected: TerminalEnvironmentSettingsSnapshot,
) {
  const terminalKinds = availableTerminalResults(probe).map((terminal) => terminal.kind);
  if (
    settings.temporaryCliTerminalKind === expected.temporaryCliTerminalKind
    && terminalKinds.length > 0
    && !terminalKinds.includes(settings.temporaryCliTerminalKind)
  ) {
    settings.temporaryCliTerminalKind = terminalKinds[0];
  }
}

export type CliEnvironmentSettingsSnapshot = Pick<
  AppSettings,
  | "codexCliPath"
  | "claudeCliPath"
  | "livenessCliKind"
>;

export function captureCliEnvironmentSettings(
  settings: AppSettings,
): CliEnvironmentSettingsSnapshot {
  return {
    codexCliPath: settings.codexCliPath,
    claudeCliPath: settings.claudeCliPath,
    livenessCliKind: settings.livenessCliKind,
  };
}

/** 把设置页手动扫描结果写回发起扫描时的草稿快照，避免覆盖扫描期间的用户修改。 */
export function applyCliEnvironmentProbeResult(
  settings: AppSettings,
  probe: CliEnvironmentProbeResult,
  expected: CliEnvironmentSettingsSnapshot,
) {
  if (
    settings.codexCliPath === expected.codexCliPath
    && probe.codex.available
    && probe.codex.path.trim()
  ) {
    settings.codexCliPath = probe.codex.path;
  }
  if (
    settings.claudeCliPath === expected.claudeCliPath
    && probe.claudeCode.available
    && probe.claudeCode.path.trim()
  ) {
    settings.claudeCliPath = probe.claudeCode.path;
  }

  const cliKinds = availableCliKinds(probe);
  if (
    settings.livenessCliKind === expected.livenessCliKind
    && cliKinds.length > 0
    && !cliKinds.includes(settings.livenessCliKind)
  ) {
    settings.livenessCliKind = cliKinds[0];
  }
}
