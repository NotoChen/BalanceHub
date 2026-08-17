import type {
  AgentCliCapabilities,
  AppSettings,
  CliEnvironmentProbeResult,
  CliToolProbeResult,
  AgentCliKind,
  TemporaryCliSessionMode,
  TemporaryCliTerminalKind,
  TerminalEnvironmentProbeResult,
  TemporaryTerminalProbeResult,
} from "../stores/providers";
import { hasAgentCliVisual } from "../agent-cli/visuals.ts";
import type { SelectOption } from "./liveness-options";

export function providerAgentBaseUrl(
  provider: { identity: { baseUrl: string }; liveness: { agentBaseUrls?: Partial<Record<AgentCliKind, string>> } },
  cliKind: AgentCliKind,
) {
  return provider.liveness.agentBaseUrls?.[cliKind]?.trim() || provider.identity.baseUrl.trim();
}

export function isAgentCliKind(value: string | null | undefined): value is AgentCliKind {
  return Boolean(value && hasAgentCliVisual(value));
}

export function agentCliTool(
  probe: CliEnvironmentProbeResult | null | undefined,
  cliKind: AgentCliKind,
): CliToolProbeResult | null {
  return probe?.tools.find((tool) => tool.kind === cliKind) || null;
}

export function agentCliLabel(
  probe: CliEnvironmentProbeResult | null | undefined,
  cliKind: AgentCliKind,
) {
  return agentCliTool(probe, cliKind)?.label || cliKind;
}

export function agentCliVersionLabel(value: string) {
  const version = value.trim();
  if (!version) return "";
  return version.match(/(?:^|[^0-9])(\d+(?:\.\d+){1,3}(?:[-+][0-9A-Za-z.-]+)?)/)?.[1] || version;
}

export function canNameSessionAtLaunch(
  probe: CliEnvironmentProbeResult | null | undefined,
  cliKind: AgentCliKind,
  sessionMode: TemporaryCliSessionMode,
) {
  if (sessionMode !== "new" || !probe) return false;
  const tool = agentCliTool(probe, cliKind);
  return Boolean(tool?.available && tool.capabilities.sessionName);
}

export function availableCliKinds(
  probe: CliEnvironmentProbeResult | null | undefined,
  capability?: keyof AgentCliCapabilities,
): AgentCliKind[] {
  if (!probe) return [];
  return probe.tools
    .filter((tool) => tool.available && (!capability || tool.capabilities[capability]))
    .map((tool) => tool.kind);
}

export function registeredCliTools(
  probe: CliEnvironmentProbeResult | null | undefined,
  capability?: keyof AgentCliCapabilities,
): CliToolProbeResult[] {
  if (!probe) return [];
  return probe.tools.filter((tool) => !capability || tool.capabilities[capability]);
}

export function availableCliOptions(
  probe: CliEnvironmentProbeResult | null | undefined,
  capability?: keyof AgentCliCapabilities,
): SelectOption<AgentCliKind>[] {
  if (!probe) return [];
  return probe.tools
    .filter((tool) => tool.available && (!capability || tool.capabilities[capability]))
    .map((tool) => ({ value: tool.kind, label: tool.label }));
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

export interface CliEnvironmentSettingsSnapshot {
  agentCliPaths: Partial<Record<AgentCliKind, string>>;
  livenessCliKind: AgentCliKind;
}

export function captureCliEnvironmentSettings(
  settings: AppSettings,
): CliEnvironmentSettingsSnapshot {
  return {
    agentCliPaths: { ...settings.agentCliPaths },
    livenessCliKind: settings.livenessCliKind,
  };
}

/** 把设置页手动扫描结果写回发起扫描时的草稿快照，避免覆盖扫描期间的用户修改。 */
export function applyCliEnvironmentProbeResult(
  settings: AppSettings,
  probe: CliEnvironmentProbeResult,
  expected: CliEnvironmentSettingsSnapshot,
) {
  for (const tool of probe.tools) {
    const currentPath = settings.agentCliPaths[tool.kind] || "";
    const expectedPath = expected.agentCliPaths[tool.kind] || "";
    if (currentPath === expectedPath && tool.available && tool.path.trim()) {
      settings.agentCliPaths[tool.kind] = tool.path;
    }
  }

  const cliKinds = availableCliKinds(probe, "liveness");
  if (
    settings.livenessCliKind === expected.livenessCliKind
    && cliKinds.length > 0
    && !cliKinds.includes(settings.livenessCliKind)
  ) {
    settings.livenessCliKind = cliKinds[0];
  }
}
