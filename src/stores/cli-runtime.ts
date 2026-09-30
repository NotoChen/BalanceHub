import { defineStore } from "pinia";
import {
  clearCliSessionIndex as clearCliSessionIndexCommand,
  activateAgentRuntime as activateAgentRuntimeCommand,
  getCliRuntimeSnapshot as getCliRuntimeSnapshotCommand,
  getCachedCliTools,
  getAgentRuntimeSnapshot as getAgentRuntimeSnapshotCommand,
  getCliSessionIndexStatus as getCliSessionIndexStatusCommand,
  getTemporaryCliInstance as getTemporaryCliInstanceCommand,
  launchTemporaryCli as launchTemporaryCliCommand,
  previewCliConfig as previewCliConfigCommand,
  previewTemporaryCliLaunch as previewTemporaryCliLaunchCommand,
  probeCliTools as probeCliToolsCommand,
  probeTerminals as probeTerminalsCommand,
} from "../api/app";
import { useWorkspaceStore } from "./workspaces";
import { useSettingsStore } from "./settings";
import type { AgentConfigurationEdit } from "./agent-configuration-types";
import { acceptsAgentRuntimeSnapshot } from "../utils/agent-runtime";
import { withTimeout } from "../utils/promise-timeout";
import type {
  AgentCliKind,
  AgentRuntimeSnapshot,
  CliEnvironmentProbeResult,
  CliRuntimeSnapshot,
  CliSessionIndexStatus,
  CliToolProbeResult,
  TemporaryCliLaunchInput,
  TemporaryCliLaunchPreview,
  TemporaryCliLaunchResult,
  TerminalEnvironmentProbeResult,
} from "./provider-types";

const pendingProbes = new WeakMap<object, { key: string; requestId: number; promise: Promise<CliEnvironmentProbeResult> }>();

export const useCliRuntimeStore = defineStore("cliRuntime", {
  state: () => ({
    cliRuntimeLoading: false,
    cliRuntime: emptyCliRuntimeSnapshot(),
    agentRuntimeSnapshot: emptyAgentRuntimeSnapshot(),
    agentRuntimeLoading: false,
    agentRuntimeRequestId: 0,
    cliEnvironmentProbe: null as CliEnvironmentProbeResult | null,
    cliEnvironmentLoading: false,
    cliEnvironmentRequestId: 0,
    terminalEnvironmentProbe: null as TerminalEnvironmentProbeResult | null,
    terminalEnvironmentLoading: false,
    terminalEnvironmentRequestId: 0,
  }),
  actions: {
    resetRuntime() {
      this.cliRuntime = emptyCliRuntimeSnapshot();
      this.agentRuntimeSnapshot = emptyAgentRuntimeSnapshot();
    },
    acceptCliToolProbe(tool: CliToolProbeResult) {
      if (this.cliEnvironmentLoading) return;
      const tools = [...(this.cliEnvironmentProbe?.tools ?? [])];
      const index = tools.findIndex((candidate) => candidate.kind === tool.kind);
      if (index < 0) tools.push(tool); else tools[index] = tool;
      this.cliEnvironmentProbe = { tools };
    },
    probeCliTools(deep = false): Promise<CliEnvironmentProbeResult> {
      const key = JSON.stringify([deep, useSettingsStore().settings.agentCliPaths]);
      const pending = pendingProbes.get(this);
      if (pending?.key === key && pending.requestId === this.cliEnvironmentRequestId) return pending.promise;
      const requestId = ++this.cliEnvironmentRequestId;
      this.cliEnvironmentLoading = deep || !this.cliEnvironmentProbe;
      const promise = (async () => {
        try {
          if (!deep && !this.cliEnvironmentProbe) {
            const cached = await withTimeout(getCachedCliTools(), 5_000, "读取 CLI 摘要超时").catch(() => null);
            if (cached && requestId === this.cliEnvironmentRequestId && !this.cliEnvironmentProbe) {
              this.cliEnvironmentProbe = cached;
              this.cliEnvironmentLoading = false;
            }
          }
          const result = await withTimeout(probeCliToolsCommand(deep), deep ? 60_000 : 30_000, "检测 Agent CLI 超时");
          if (this.cliEnvironmentRequestId === requestId) this.cliEnvironmentProbe = result;
          return result;
        } finally {
          if (this.cliEnvironmentRequestId === requestId) this.cliEnvironmentLoading = false;
          if (pendingProbes.get(this)?.requestId === requestId) pendingProbes.delete(this);
        }
      })();
      pendingProbes.set(this, { key, requestId, promise });
      return promise;
    },
    cancelCliToolsProbe(requestId?: number) {
      if (requestId !== undefined && this.cliEnvironmentRequestId !== requestId) {
        return false;
      }
      this.cliEnvironmentRequestId += 1;
      this.cliEnvironmentLoading = false;
      return true;
    },
    async probeTerminals() {
      const requestId = ++this.terminalEnvironmentRequestId;
      this.terminalEnvironmentLoading = true;
      try {
        const result = await probeTerminalsCommand();
        if (requestId === this.terminalEnvironmentRequestId) this.terminalEnvironmentProbe = result;
        return result;
      } finally {
        if (requestId === this.terminalEnvironmentRequestId) this.terminalEnvironmentLoading = false;
      }
    },
    cancelTerminalsProbe(requestId?: number) {
      if (requestId !== undefined && requestId !== this.terminalEnvironmentRequestId) return false;
      this.terminalEnvironmentRequestId += 1;
      this.terminalEnvironmentLoading = false;
      return true;
    },
    async launch(input: TemporaryCliLaunchInput): Promise<TemporaryCliLaunchResult> {
      const result = await launchTemporaryCliCommand(input);
      useWorkspaceStore().recordLaunch(result);
      return result;
    },
    async previewLaunch(input: TemporaryCliLaunchInput): Promise<TemporaryCliLaunchPreview> {
      return previewTemporaryCliLaunchCommand(input);
    },
    async getSessionIndexStatus(): Promise<CliSessionIndexStatus> {
      return getCliSessionIndexStatusCommand();
    },
    async clearSessionIndex(): Promise<void> {
      return clearCliSessionIndexCommand();
    },
    acceptAgentRuntimeSnapshot(snapshot: AgentRuntimeSnapshot) {
      if (!acceptsAgentRuntimeSnapshot(this.agentRuntimeSnapshot, snapshot)) {
        return false;
      }
      this.agentRuntimeSnapshot = snapshot;
      return true;
    },
    async refreshAgentRuntimeSnapshot(): Promise<AgentRuntimeSnapshot> {
      const requestId = ++this.agentRuntimeRequestId;
      this.agentRuntimeLoading = true;
      try {
        const snapshot = await getAgentRuntimeSnapshotCommand();
        if (this.agentRuntimeRequestId === requestId) {
          this.acceptAgentRuntimeSnapshot(snapshot);
        }
        return snapshot;
      } finally {
        if (this.agentRuntimeRequestId === requestId) {
          this.agentRuntimeLoading = false;
        }
      }
    },
    cancelAgentRuntimeRefresh() {
      this.agentRuntimeRequestId += 1;
      this.agentRuntimeLoading = false;
    },
    async activateAgentRuntime(runtimeId: string) {
      await activateAgentRuntimeCommand(runtimeId);
    },
    async getInstance(instanceId: string) {
      return getTemporaryCliInstanceCommand(instanceId);
    },
    async previewConfig(
      id: string,
      cliKind: AgentCliKind,
      apiKeyLocalId: string,
    ): Promise<AgentConfigurationEdit> {
      return previewCliConfigCommand(id, cliKind, apiKeyLocalId);
    },
    async refresh(): Promise<CliRuntimeSnapshot> {
      this.cliRuntimeLoading = true;
      try {
        this.cliRuntime = await withTimeout(getCliRuntimeSnapshotCommand(), 15_000, "读取 CLI 配置状态超时");
        return this.cliRuntime;
      } finally {
        this.cliRuntimeLoading = false;
      }
    },
  },
});

function emptyCliRuntimeSnapshot(): CliRuntimeSnapshot {
  return {
    agents: [],
    configs: [],
  };
}

function emptyAgentRuntimeSnapshot(): AgentRuntimeSnapshot {
  return {
    schemaVersion: 1,
    revision: 0,
    updatedAt: 0,
    sessions: [],
  };
}
