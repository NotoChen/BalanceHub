import type { AgentCliKind, AgentRuntimeSession, TemporaryCliInstance } from "../src/stores/provider-types.ts";
import type { AgentSessionDetail, AgentSessionLaunchIdentity, AgentSessionPage, AgentSessionResumeOperation, AgentSessionRow, AgentSessionScope } from "../src/stores/agent-session-types.ts";

export function sessionScope(explicitWorkdir: string | null = null): AgentSessionScope {
  return {
    revision: explicitWorkdir ? `scope:${explicitWorkdir}` : "scope:recorded",
    workspaces: explicitWorkdir
      ? [{ id: `workspace:${explicitWorkdir}`, path: explicitWorkdir, exists: true, isHome: false }]
      : [
        { id: "home", path: "/fixture/home", exists: true, isHome: true },
        { id: "project", path: "/fixture/home/project", exists: true, isHome: false },
        { id: "unmounted", path: "/fixture/unmounted/project", exists: false, isHome: false },
      ],
    sources: [
      { id: "source:codex", agentKind: "codex", configRoot: "/fixture/native/codex", available: true },
      { id: "source:claude", agentKind: "claudeCode", configRoot: "/fixture/native/claude", available: true },
    ],
  };
}

export function sessionRow(id = "session-1", overrides: Partial<AgentSessionRow> = {}): AgentSessionRow {
  return {
    sessionRef: `ref:${id}`, sourceId: "source:codex", workspaceId: "home",
    session: {
      id, title: `合成会话 ${id}`, preview: "合成的用户与助手正文摘要", model: "fixture-model", models: ["fixture-model"],
      cliKind: "codex", createdAt: "2026-09-01T01:00:00Z", updatedAt: "2026-09-01T02:00:00Z", workdir: "/fixture/home",
      cliVersion: null, archived: false, canResume: true, metadataSource: "fixture",
    },
    role: "main", parent: { kind: "none" }, resumeReason: null, activityState: "unknown", runtimeIds: [], ...overrides,
  };
}

export function sessionPage(items: AgentSessionRow[] = [], overrides: Partial<AgentSessionPage> = {}): AgentSessionPage {
  return {
    snapshotId: "snapshot:1", scopeRevision: "scope:recorded", items, parentUpdates: [], nextCursor: null,
    loadedCount: items.length, total: items.length, agentCounts: [],
    sourceStates: [{ sourceId: "source:codex", workspaceId: "home", state: "complete", loadedCount: items.length, message: null, indexState: "ready" }],
    ...overrides,
  };
}

export function sessionDetail(row = sessionRow(), overrides: Partial<AgentSessionDetail> = {}): AgentSessionDetail {
  return {
    row,
    detail: {
      session: row.session,
      messages: [
        { id: "message-1", role: "user", content: "合成用户正文", timestamp: null, model: null, toolName: null },
        { id: "message-2", role: "assistant", content: "合成助手正文", timestamp: null, model: "fixture-model", toolName: null },
        { id: "message-3", role: "tool", content: "合成工具记录", timestamp: null, model: null, toolName: "fixture-tool" },
      ],
      truncated: false, omittedMessageCount: 0, contentSource: "fixture",
    },
    ...overrides,
  };
}

export function sessionLaunchIdentity(row = sessionRow()): AgentSessionLaunchIdentity {
  return { sessionRef: row.sessionRef, sourceIdentity: row.sourceId, nativeSessionId: row.session.id, runtimeScope: { kind: "native" } };
}

export function sessionInstance(overrides: Partial<TemporaryCliInstance> = {}): TemporaryCliInstance {
  return {
    id: "instance:1", providerId: null, providerName: null, nativeSession: sessionLaunchIdentity(), apiKeyLocalId: null,
    sessionTitle: "合成原生继续", accountLabel: "", cliKind: "codex", workdir: "/fixture/home", terminalKind: "terminal",
    terminalName: "系统终端", terminalLocator: "fixture-terminal", startedAt: "2026-09-01T02:00:00Z", endedAt: null,
    pid: 123, status: "running", exitCode: null, canActivate: true, ...overrides,
  };
}

export function resumeOperation(overrides: Partial<AgentSessionResumeOperation> = {}): AgentSessionResumeOperation {
  return {
    id: "operation:1", revision: 1, requestId: "request:1", sessionRef: "ref:session-1", cliKind: "codex",
    state: "queued", message: "等待合成启动", createdAt: "2026-09-01T02:00:00Z", updatedAt: "2026-09-01T02:00:01Z",
    canCancel: true, runtimeIds: [], result: null, ...overrides,
  };
}

export function runtimeSession(id = "runtime:1", kind: AgentCliKind = "codex", overrides: Partial<AgentRuntimeSession> = {}): AgentRuntimeSession {
  return {
    runtimeId: id, nativeSession: sessionLaunchIdentity(), runtimeScope: { kind: "native" }, origin: "balancehub_launch", agentKind: kind,
    agentSessionId: "session-1", balancehubInstanceId: "instance:1", provider: null, workdir: "/fixture/home", title: "合成会话",
    model: null, process: null, terminal: null, state: "idle", evidence: [], startedAt: 1, lastActivityAt: 2, endedAt: null, exitCode: null,
    actions: { canActivateTerminal: false, canViewDetail: true, canResume: false, canDismiss: false }, ...overrides,
  };
}
