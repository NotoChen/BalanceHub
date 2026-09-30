import type {
  AgentCliKind,
  AgentRuntimeOrigin,
  AgentRuntimeScope,
  AgentRuntimeSession,
  AgentRuntimeSnapshot,
  AgentRuntimeState,
  TemporaryCliTerminalKind,
} from "../stores/provider-types";

export type AgentHookTargetKey = `${AgentCliKind}:${string}`;

export function agentRuntimeScopeKey(scope: AgentRuntimeScope = { kind: "native" }) {
  return scope.kind === "wsl" ? `wsl:${scope.distro_id}` : "native";
}

export function agentHookTargetKey(
  agentKind: AgentCliKind,
  scope: AgentRuntimeScope = { kind: "native" },
): AgentHookTargetKey {
  return `${agentKind}:${agentRuntimeScopeKey(scope)}` as AgentHookTargetKey;
}

const runtimeStateLabels: Record<AgentRuntimeState, string> = {
  starting: "正在启动",
  busy: "正在响应",
  idle: "等待输入",
  ended: "已结束",
  unknown: "状态未知",
};

const runtimeOriginLabels: Record<AgentRuntimeOrigin, string> = {
  balancehub_launch: "BalanceHub 启动",
  external_hook: "外部终端发现",
};

const terminalLabels: Record<TemporaryCliTerminalKind, string> = {
  terminal: "系统终端",
  iTerm2: "iTerm2",
  ghostty: "Ghostty",
  warp: "Warp",
  wezTerm: "WezTerm",
  kitty: "Kitty",
  alacritty: "Alacritty",
  kaku: "Kaku",
  windowsTerminal: "Windows Terminal",
  powerShell: "PowerShell",
  commandPrompt: "命令提示符",
};

export function activeAgentRuntimeSessions(snapshot: AgentRuntimeSnapshot) {
  return snapshot.sessions
    .filter((session) => session.state !== "ended")
    .sort((left, right) => runtimeSortTimestamp(right) - runtimeSortTimestamp(left));
}

/** Unknown records stay visible but never inflate confirmed activity counts. */
export function isConfirmedAgentRuntimeSession(session: AgentRuntimeSession) {
  return session.state !== "unknown" && session.state !== "ended";
}

export function acceptsAgentRuntimeSnapshot(
  current: AgentRuntimeSnapshot,
  incoming: AgentRuntimeSnapshot,
) {
  return incoming.revision >= current.revision;
}

export function runtimeStateLabel(state: AgentRuntimeState) {
  return runtimeStateLabels[state];
}

export function runtimeOriginLabel(origin: AgentRuntimeOrigin) {
  return runtimeOriginLabels[origin];
}

export function runtimeTerminalLabel(kind: TemporaryCliTerminalKind | undefined) {
  return kind ? terminalLabels[kind] : "终端未知";
}

export function runtimeSessionTitle(session: AgentRuntimeSession) {
  return session.title?.trim() || "未命名会话";
}

export function runtimeWorkdirName(workdir: string | null) {
  const path = workdir?.trim() ?? "";
  if (!path) return "目录未知";
  const normalized = path.replace(/[\\/]+$/, "");
  if (!normalized) return path;
  const segments = normalized.split(/[\\/]/).filter(Boolean);
  return segments[segments.length - 1] || normalized;
}

export function runtimeSortTimestamp(session: AgentRuntimeSession) {
  return session.lastActivityAt ?? session.startedAt ?? 0;
}
