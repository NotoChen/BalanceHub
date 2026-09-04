import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";
import type { AgentRuntimeSession, AgentRuntimeSnapshot } from "../src/stores/provider-types.ts";
import {
  activeAgentRuntimeSessions,
  acceptsAgentRuntimeSnapshot,
  runtimeOriginLabel,
  runtimeSessionTitle,
  runtimeStateLabel,
  runtimeTerminalLabel,
  runtimeWorkdirName,
} from "../src/utils/agent-runtime.ts";

test("unified runtime activity and order come only from Rust projection facts", () => {
  const snapshot: AgentRuntimeSnapshot = {
    schemaVersion: 1,
    revision: 2,
    updatedAt: 30,
    sessions: [
      sessionFixture("ended", "ended", 30),
      sessionFixture("older", "idle", 10),
      sessionFixture("newer", "busy", 20),
    ],
  };

  assert.deepEqual(
    activeAgentRuntimeSessions(snapshot).map((session) => session.runtimeId),
    ["newer", "older"],
  );
});

test("unified runtime labels preserve unknown evidence instead of inventing facts", () => {
  const session = sessionFixture("external", "unknown", 0);
  assert.equal(runtimeOriginLabel(session.origin), "外部终端发现");
  assert.equal(runtimeStateLabel(session.state), "状态未知");
  assert.equal(runtimeTerminalLabel(session.terminal?.kind), "终端未知");
  assert.equal(runtimeSessionTitle(session), "未命名会话");
  assert.equal(runtimeWorkdirName(session.workdir), "目录未知");
});

test("runtime snapshots reject older revisions while accepting equal event delivery", () => {
  const current = snapshotFixture(8);
  assert.equal(acceptsAgentRuntimeSnapshot(current, snapshotFixture(7)), false);
  assert.equal(acceptsAgentRuntimeSnapshot(current, snapshotFixture(8)), true);
  assert.equal(acceptsAgentRuntimeSnapshot(current, snapshotFixture(9)), true);
});

test("runtime UI uses push and resume calibration instead of the legacy list poll", () => {
  const composable = readFileSync(join(process.cwd(), "src/composables/useCliRuntime.ts"), "utf8");
  const appApi = readFileSync(join(process.cwd(), "src/api/app.ts"), "utf8");
  const runtimeStore = readFileSync(join(process.cwd(), "src/stores/cli-runtime.ts"), "utf8");
  const providerTypes = readFileSync(join(process.cwd(), "src/stores/provider-types.ts"), "utf8");
  assert.match(composable, /agent-runtime-updated/);
  assert.match(composable, /visibilitychange/);
  assert.match(composable, /removeEventListener/);
  assert.doesNotMatch(composable, /getTemporaryCliInstances|instancePollTimer|4_000/);
  assert.match(appApi, /get_agent_runtime_snapshot/);
  assert.match(appApi, /activate_agent_runtime/);
  assert.doesNotMatch(appApi, /get_temporary_cli_instances/);
  assert.doesNotMatch(appApi, /activate_temporary_cli/);
  assert.doesNotMatch(runtimeStore, /cliRuntime\.instances/);
  assert.doesNotMatch(
    providerTypes.match(/export interface CliRuntimeSnapshot \{[\s\S]*?\n\}/)?.[0] ?? "",
    /instances:/,
  );
});

function snapshotFixture(revision: number): AgentRuntimeSnapshot {
  return {
    schemaVersion: 1,
    revision,
    updatedAt: revision,
    sessions: [],
  };
}

function sessionFixture(
  runtimeId: string,
  state: AgentRuntimeSession["state"],
  lastActivityAt: number,
): AgentRuntimeSession {
  return {
    runtimeId,
    runtimeScope: { kind: "native" },
    origin: "external_hook",
    agentKind: "codex",
    agentSessionId: null,
    balancehubInstanceId: null,
    provider: null,
    workdir: null,
    title: null,
    model: null,
    process: null,
    terminal: null,
    state,
    evidence: [],
    startedAt: lastActivityAt,
    lastActivityAt,
    endedAt: state === "ended" ? lastActivityAt : null,
    exitCode: null,
    actions: {
      canActivateTerminal: false,
      canViewDetail: true,
      canResume: false,
      canDismiss: true,
    },
  };
}
