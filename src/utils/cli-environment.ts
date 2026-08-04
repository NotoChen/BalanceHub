import type {
  AppSettings,
  CliEnvironmentProbeResult,
  LivenessCliKind,
  TemporaryCliSessionMode,
  TemporaryCliTerminalKind,
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
  probe: CliEnvironmentProbeResult | null | undefined,
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
  probe: CliEnvironmentProbeResult | null | undefined,
): SelectOption<TemporaryCliTerminalKind>[] {
  return availableTerminalResults(probe).map((terminal) => ({
    value: terminal.kind,
    label: terminal.name,
  }));
}

export function applyCliEnvironmentDefaults(
  settings: AppSettings,
  probe: CliEnvironmentProbeResult,
) {
  const cliKinds = availableCliKinds(probe);
  if (cliKinds.length > 0 && !cliKinds.includes(settings.livenessCliKind)) {
    settings.livenessCliKind = cliKinds[0];
  }

  const terminalKinds = availableTerminalResults(probe).map((terminal) => terminal.kind);
  if (
    terminalKinds.length > 0 &&
    !terminalKinds.includes(settings.temporaryCliTerminalKind)
  ) {
    settings.temporaryCliTerminalKind = terminalKinds[0];
  }
}

export type CliEnvironmentSettingsSnapshot = Pick<
  AppSettings,
  | "codexCliPath"
  | "claudeCliPath"
  | "livenessCliKind"
  | "temporaryCliTerminalKind"
>;

export function captureCliEnvironmentSettings(
  settings: AppSettings,
): CliEnvironmentSettingsSnapshot {
  return {
    codexCliPath: settings.codexCliPath,
    claudeCliPath: settings.claudeCliPath,
    livenessCliKind: settings.livenessCliKind,
    temporaryCliTerminalKind: settings.temporaryCliTerminalKind,
  };
}

/**
 * 把扫描结果写回发起扫描时的设置快照。扫描期间用户已经修改的字段不再覆盖，
 * 其余字段仍可跟随 CLI 升级、版本管理器目录迁移和终端安装变化自动更新。
 */
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

  const next = { ...settings };
  applyCliEnvironmentDefaults(next, probe);
  if (settings.livenessCliKind === expected.livenessCliKind) {
    settings.livenessCliKind = next.livenessCliKind;
  }
  if (settings.temporaryCliTerminalKind === expected.temporaryCliTerminalKind) {
    settings.temporaryCliTerminalKind = next.temporaryCliTerminalKind;
  }
}
