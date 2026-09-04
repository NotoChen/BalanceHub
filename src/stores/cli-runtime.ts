import { defineStore } from "pinia";
import {
  clearCliSessionIndex as clearCliSessionIndexCommand,
  activateAgentRuntime as activateAgentRuntimeCommand,
  getCliRuntimeSnapshot as getCliRuntimeSnapshotCommand,
  getAgentRuntimeSnapshot as getAgentRuntimeSnapshotCommand,
  getCliSessionIndexStatus as getCliSessionIndexStatusCommand,
  getCliSessionDetail as getCliSessionDetailCommand,
  getTemporaryCliInstance as getTemporaryCliInstanceCommand,
  launchTemporaryCli as launchTemporaryCliCommand,
  previewCliConfig as previewCliConfigCommand,
  previewTemporaryCliLaunch as previewTemporaryCliLaunchCommand,
  probeCliTools as probeCliToolsCommand,
  probeTerminals as probeTerminalsCommand,
  searchCliSessions as searchCliSessionsCommand,
  switchCliConfig as switchCliConfigCommand,
} from "../api/app";
import { useWorkspaceStore } from "./workspaces";
import { acceptsAgentRuntimeSnapshot } from "../utils/agent-runtime";
import type {
  AgentCliKind,
  AgentRuntimeSnapshot,
  CliConfigFile,
  CliConfigPreview,
  CliEnvironmentProbeResult,
  CliRuntimeSnapshot,
  CliSessionDetail,
  CliSessionIndexStatus,
  CliSessionSearchResponse,
  TemporaryCliLaunchInput,
  TemporaryCliLaunchPreview,
  TemporaryCliLaunchResult,
  TerminalEnvironmentProbeResult,
} from "./provider-types";

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
  }),
  actions: {
    resetRuntime() {
      this.cliRuntime = emptyCliRuntimeSnapshot();
      this.agentRuntimeSnapshot = emptyAgentRuntimeSnapshot();
    },
    async probeCliTools(deep = false) {
      const requestId = ++this.cliEnvironmentRequestId;
      this.cliEnvironmentLoading = true;
      try {
        const result = await probeCliToolsCommand(deep);
        // A late result may belong to a cancelled or superseded probe. It may
        // still be returned to its caller, but must not replace the shared
        // probe snapshot used by other settings controls.
        if (this.cliEnvironmentRequestId === requestId) {
          this.cliEnvironmentProbe = result;
        }
        return result;
      } finally {
        if (this.cliEnvironmentRequestId === requestId) {
          this.cliEnvironmentLoading = false;
        }
      }
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
      this.terminalEnvironmentLoading = true;
      try {
        const result = await probeTerminalsCommand();
        this.terminalEnvironmentProbe = result;
        return result;
      } finally {
        this.terminalEnvironmentLoading = false;
      }
    },
    async launch(input: TemporaryCliLaunchInput): Promise<TemporaryCliLaunchResult> {
      const result = await launchTemporaryCliCommand(input);
      useWorkspaceStore().recordLaunch(result);
      return result;
    },
    async previewLaunch(input: TemporaryCliLaunchInput): Promise<TemporaryCliLaunchPreview> {
      return previewTemporaryCliLaunchCommand(input);
    },
    async searchSessions(
      cliKind: AgentCliKind,
      workdir: string,
      query: string,
      forceRefresh = false,
    ): Promise<CliSessionSearchResponse> {
      return searchCliSessionsCommand(cliKind, workdir, query, 50, forceRefresh);
    },
    async getSessionIndexStatus(): Promise<CliSessionIndexStatus> {
      return getCliSessionIndexStatusCommand();
    },
    async clearSessionIndex(): Promise<void> {
      return clearCliSessionIndexCommand();
    },
    async getSessionDetail(
      cliKind: AgentCliKind,
      workdir: string,
      sessionId: string,
    ): Promise<CliSessionDetail> {
      return getCliSessionDetailCommand(cliKind, workdir, sessionId);
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
    ): Promise<CliConfigPreview> {
      return previewCliConfigCommand(id, cliKind, apiKeyLocalId);
    },
    async switchConfig(
      id: string,
      cliKind: AgentCliKind,
      apiKeyLocalId: string,
      revision: string,
      files: CliConfigFile[],
    ) {
      return switchCliConfigCommand(
        id,
        cliKind,
        apiKeyLocalId,
        revision,
        files,
      );
    },
    async refresh(): Promise<CliRuntimeSnapshot> {
      this.cliRuntimeLoading = true;
      try {
        this.cliRuntime = await getCliRuntimeSnapshotCommand();
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
