import assert from "node:assert/strict";
import test from "node:test";

import type {
  CliEnvironmentProbeResult,
  TemporaryCliInstance,
  TemporaryCliInstanceStatus,
} from "../src/stores/provider-types.ts";
import { canNameSessionAtLaunch } from "../src/utils/cli-environment.ts";
import { waitForTemporaryCliStart } from "../src/utils/temporary-cli-launch.ts";

function instance(status: TemporaryCliInstanceStatus, exitCode: number | null = null) {
  return {
    id: "instance-1",
    providerId: "provider-1",
    providerName: "Relay",
    cliKind: "codex",
    workdir: "/workspace",
    terminalKind: "terminal",
    terminalName: "系统终端",
    startedAt: "1",
    endedAt: status === "exited" ? "2" : null,
    pid: status === "running" ? 123 : null,
    status,
    exitCode,
    canActivate: false,
  } satisfies TemporaryCliInstance;
}

test("temporary CLI launch waits for a stable running state", async () => {
  let clock = 0;
  const statuses: TemporaryCliInstanceStatus[] = ["starting", "running", "running", "running"];

  const result = await waitForTemporaryCliStart(
    "instance-1",
    async () => instance(statuses.shift() ?? "running"),
    {
      timeoutMs: 1_000,
      pollIntervalMs: 100,
      stableForMs: 200,
      now: () => clock,
      wait: async (milliseconds) => {
        clock += milliseconds;
      },
    },
  );

  assert.equal(result.status, "running");
});

test("temporary CLI launch reports an immediate process exit", async () => {
  await assert.rejects(
    waitForTemporaryCliStart("instance-1", async () => instance("exited", 17), {
      timeoutMs: 1_000,
      stableForMs: 0,
    }),
    /退出码 17/,
  );
});

test("temporary CLI launch timeout is bounded", async () => {
  let clock = 0;
  let reads = 0;

  await assert.rejects(
    waitForTemporaryCliStart(
      "instance-1",
      async () => {
        reads += 1;
        return null;
      },
      {
        timeoutMs: 250,
        pollIntervalMs: 100,
        now: () => clock,
        wait: async (milliseconds) => {
          clock += milliseconds;
        },
      },
    ),
    /未收到临时 CLI 启动确认/,
  );
  assert.equal(reads, 3);
});

test("launch-time session naming follows the CLI capability boundary", () => {
  const probe = {
    tools: [
      {
        kind: "codex",
        label: "Codex CLI",
        executable: "codex",
        sessionNameHint: "Codex CLI 当前不支持启动前命名",
        capabilities: {
          temporaryLaunch: true,
          modelSelection: true,
          sessionHistory: true,
          sessionResume: true,
          sessionName: false,
          liveness: true,
          defaultConfig: true,
        },
        available: true,
        path: "/usr/local/bin/codex",
        version: "0.146.0",
        message: "",
      },
      {
        kind: "claudeCode",
        label: "Claude Code",
        executable: "claude",
        sessionNameHint: "",
        capabilities: {
          temporaryLaunch: true,
          modelSelection: true,
          sessionHistory: true,
          sessionResume: true,
          sessionName: true,
          liveness: true,
          defaultConfig: true,
        },
        available: true,
        path: "/usr/local/bin/claude",
        version: "2.1.221",
        message: "",
      },
    ],
  } satisfies CliEnvironmentProbeResult;

  assert.equal(canNameSessionAtLaunch(probe, "claudeCode", "new"), true);
  assert.equal(canNameSessionAtLaunch(probe, "claudeCode", "history"), false);
  assert.equal(canNameSessionAtLaunch(probe, "codex", "new"), false);
});
