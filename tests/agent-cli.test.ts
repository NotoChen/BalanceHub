import assert from "node:assert/strict";
import test from "node:test";

import type {
  AppSettings,
  CliEnvironmentProbeResult,
} from "../src/stores/provider-types.ts";
import {
  agentCliVersionLabel,
  applyCliEnvironmentProbeResult,
  availableCliOptions,
  captureCliEnvironmentSettings,
  registeredCliTools,
} from "../src/utils/cli-environment.ts";

test("Agent CLI version labels keep only the official version number", () => {
  assert.equal(agentCliVersionLabel("codex-cli 0.147.0"), "0.147.0");
  assert.equal(agentCliVersionLabel("2.1.232 (Claude Code)"), "2.1.232");
  assert.equal(agentCliVersionLabel("0.55.1"), "0.55.1");
  assert.equal(agentCliVersionLabel("grok 1.0.3 (1a29d5bc12d4)"), "1.0.3");
});

const probe = {
  tools: [
    {
      kind: "claudeCode",
      label: "Claude Code",
      executable: "claude",
      sessionNameHint: "",
      capabilities: {
        temporaryLaunch: true,
        modelSelection: true,
        sessionHistory: true,
        sessionSearch: true,
        sessionDetail: true,
        sessionResume: true,
        sessionName: true,
        liveness: false,
        defaultConfig: true,
      },
      available: true,
      path: "/opt/tools/claude",
      version: "2.1.221",
      message: "",
    },
    {
      kind: "codex",
      label: "Codex CLI",
      executable: "codex",
      sessionNameHint: "Codex CLI 当前不支持启动前命名",
      capabilities: {
        temporaryLaunch: true,
        modelSelection: true,
        sessionHistory: true,
        sessionSearch: true,
        sessionDetail: true,
        sessionResume: true,
        sessionName: false,
        liveness: true,
        defaultConfig: true,
      },
      available: true,
      path: "/opt/tools/codex",
      version: "0.146.0",
      message: "",
    },
    {
      kind: "gemini",
      label: "Gemini CLI",
      executable: "gemini",
      sessionNameHint: "Gemini CLI 没有启动前会话命名参数，标题由 Gemini 自动生成",
      capabilities: {
        temporaryLaunch: true,
        modelSelection: true,
        sessionHistory: true,
        sessionSearch: true,
        sessionDetail: true,
        sessionResume: true,
        sessionName: false,
        liveness: true,
        defaultConfig: true,
      },
      available: true,
      path: "/opt/tools/gemini",
      version: "0.55.1",
      message: "",
    },
    {
      kind: "grok",
      label: "Grok Build",
      executable: "grok",
      sessionNameHint: "Grok Build 不支持启动前命名；启动后可在终端输入 /rename",
      capabilities: {
        temporaryLaunch: true,
        modelSelection: true,
        sessionHistory: true,
        sessionSearch: true,
        sessionDetail: true,
        sessionResume: true,
        sessionName: false,
        liveness: true,
        defaultConfig: true,
      },
      available: true,
      path: "/opt/tools/grok",
      version: "grok 1.0.3",
      message: "",
    },
  ],
} satisfies CliEnvironmentProbeResult;

test("Agent CLI options follow the Rust registry order and capability flags", () => {
  assert.deepEqual(availableCliOptions(probe), [
    { value: "claudeCode", label: "Claude Code" },
    { value: "codex", label: "Codex CLI" },
    { value: "gemini", label: "Gemini CLI" },
    { value: "grok", label: "Grok Build" },
  ]);
  assert.deepEqual(availableCliOptions(probe, "liveness"), [
    { value: "codex", label: "Codex CLI" },
    { value: "gemini", label: "Gemini CLI" },
    { value: "grok", label: "Grok Build" },
  ]);
  assert.deepEqual(availableCliOptions(probe, "defaultConfig"), [
    { value: "claudeCode", label: "Claude Code" },
    { value: "codex", label: "Codex CLI" },
    { value: "gemini", label: "Gemini CLI" },
    { value: "grok", label: "Grok Build" },
  ]);
});

test("registered Agent metadata does not disappear when a CLI is unavailable", () => {
  const unavailableProbe = {
    tools: probe.tools.map((tool) =>
      tool.kind === "gemini" ? { ...tool, available: false, path: "" } : tool,
    ),
  } satisfies CliEnvironmentProbeResult;

  assert.deepEqual(
    registeredCliTools(unavailableProbe).map((tool) => tool.kind),
    ["claudeCode", "codex", "gemini", "grok"],
  );
  assert.deepEqual(
    availableCliOptions(unavailableProbe).map((option) => option.value),
    ["claudeCode", "codex", "grok"],
  );
});

test("Agent CLI scan writes paths by kind without overwriting concurrent edits", () => {
  const settings = {
    agentCliPaths: {},
    livenessCliKind: "codex",
  } as AppSettings;
  const expected = captureCliEnvironmentSettings(settings);
  settings.agentCliPaths.claudeCode = "/custom/claude";

  applyCliEnvironmentProbeResult(settings, probe, expected);

  assert.equal(settings.agentCliPaths.codex, "/opt/tools/codex");
  assert.equal(settings.agentCliPaths.gemini, "/opt/tools/gemini");
  assert.equal(settings.agentCliPaths.grok, "/opt/tools/grok");
  assert.equal(settings.agentCliPaths.claudeCode, "/custom/claude");
  assert.equal(settings.livenessCliKind, "codex");
});
