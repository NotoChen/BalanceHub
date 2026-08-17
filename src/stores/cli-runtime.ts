import { defineStore } from "pinia";
import {
  activateTemporaryCli as activateTemporaryCliCommand,
  getCliRuntimeSnapshot as getCliRuntimeSnapshotCommand,
  getTemporaryCliInstance as getTemporaryCliInstanceCommand,
  getTemporaryCliInstances as getTemporaryCliInstancesCommand,
  launchTemporaryCli as launchTemporaryCliCommand,
  listCliSessions as listCliSessionsCommand,
  previewCliConfig as previewCliConfigCommand,
  previewTemporaryCliLaunch as previewTemporaryCliLaunchCommand,
  probeCliTools as probeCliToolsCommand,
  probeTerminals as probeTerminalsCommand,
  switchCliConfig as switchCliConfigCommand,
} from "../api/app";
import { useWorkspaceStore } from "./workspaces";
import type {
  AgentCliKind,
  CliConfigFile,
  CliConfigPreview,
  CliEnvironmentProbeResult,
  CliRuntimeSnapshot,
  CliSessionSummary,
  TemporaryCliLaunchInput,
  TemporaryCliLaunchPreview,
  TemporaryCliLaunchResult,
  TerminalEnvironmentProbeResult,
} from "./provider-types";

export const useCliRuntimeStore = defineStore("cliRuntime", {
  state: () => ({
    cliRuntimeLoading: false,
    cliRuntime: emptyCliRuntimeSnapshot(),
    cliEnvironmentProbe: null as CliEnvironmentProbeResult | null,
    cliEnvironmentLoading: false,
    terminalEnvironmentProbe: null as TerminalEnvironmentProbeResult | null,
    terminalEnvironmentLoading: false,
  }),
  actions: {
    resetRuntime() {
      this.cliRuntime = emptyCliRuntimeSnapshot();
    },
    async probeCliTools(deep = false) {
      this.cliEnvironmentLoading = true;
      try {
        const result = await probeCliToolsCommand(deep);
        this.cliEnvironmentProbe = result;
        return result;
      } finally {
        this.cliEnvironmentLoading = false;
      }
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
      const instances = this.cliRuntime.instances.filter(
        (instance) => instance.id !== result.instance.id,
      );
      this.cliRuntime = {
        ...this.cliRuntime,
        instances:
          result.instance.status === "exited" ? instances : [result.instance, ...instances],
      };
      return result;
    },
    async previewLaunch(input: TemporaryCliLaunchInput): Promise<TemporaryCliLaunchPreview> {
      return previewTemporaryCliLaunchCommand(input);
    },
    async listSessions(cliKind: AgentCliKind, workdir: string): Promise<CliSessionSummary[]> {
      return listCliSessionsCommand(cliKind, workdir);
    },
    async activate(instanceId: string) {
      await activateTemporaryCliCommand(instanceId);
    },
    async refreshInstances() {
      const instances = await getTemporaryCliInstancesCommand();
      this.cliRuntime = { ...this.cliRuntime, instances };
      return instances;
    },
    async getInstance(instanceId: string) {
      const instance = await getTemporaryCliInstanceCommand(instanceId);
      const remaining = this.cliRuntime.instances.filter((item) => item.id !== instanceId);
      this.cliRuntime = {
        ...this.cliRuntime,
        instances: instance && instance.status !== "exited" ? [instance, ...remaining] : remaining,
      };
      return instance;
    },
    async previewConfig(id: string, cliKind: AgentCliKind): Promise<CliConfigPreview> {
      return previewCliConfigCommand(id, cliKind);
    },
    async switchConfig(
      id: string,
      cliKind: AgentCliKind,
      revision: string,
      files: CliConfigFile[],
    ) {
      this.cliRuntime = await switchCliConfigCommand(id, cliKind, revision, files);
      return this.cliRuntime;
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
    configs: [],
    instances: [],
  };
}
