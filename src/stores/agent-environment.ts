import { defineStore } from "pinia";
import {
  checkAgentLatestVersions,
  getAgentEnvironmentInventory,
  openAgentEnvironmentAsset,
  readAgentEnvironmentAsset,
} from "../api/app";
import type {
  AgentAssetReadResult,
  AgentAssetOpenTarget,
  AgentEnvironmentInventory,
  AgentInstallation,
  AgentVersionCheckResult,
} from "./provider-types";

type AsyncState = "idle" | "loading" | "refreshing" | "ready" | "error";

export function agentEnvironmentKey(workspace?: string) {
  return workspace?.trim() || "__native__";
}

function normalizedWorkspace(workspace?: string) {
  return workspace?.trim() || undefined;
}

function previewKey(assetId: string, workspace?: string) {
  const workspaceKey = agentEnvironmentKey(workspace);
  return `${workspaceKey.length}:${workspaceKey}:${assetId}`;
}

function nextRequestId(requestIds: Record<string, number>, key: string) {
  const requestId = (requestIds[key] ?? 0) + 1;
  requestIds[key] = requestId;
  return requestId;
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function mergeVersionFacts(
  inventory: AgentEnvironmentInventory,
  versions: AgentVersionCheckResult | undefined,
): AgentEnvironmentInventory {
  if (!versions) return inventory;
  const versionsById = new Map(versions.installations.map((installation) => [installation.id, installation]));
  return {
    ...inventory,
    installations: inventory.installations.map((installation) => {
      const version = versionsById.get(installation.id);
      if (!version) return installation;
      const diagnostics = [installation.diagnostic, version.diagnostic].filter(
        (value, index, values): value is string => Boolean(value) && values.indexOf(value) === index,
      );
      return {
        ...installation,
        latestStableVersion: version.latestStableVersion,
        latestVersionSource: version.latestVersionSource,
        versionState: version.versionState,
        versionCheckedAt: version.versionCheckedAt,
        diagnostic: diagnostics.length ? diagnostics.join("；") : null,
      };
    }),
  };
}

export const useAgentEnvironmentStore = defineStore("agent-environment", {
  state: () => ({
    inventories: {} as Record<string, AgentEnvironmentInventory>,
    inventoryState: {} as Record<string, AsyncState>,
    inventoryErrors: {} as Record<string, string | null>,
    versions: {} as Record<string, AgentVersionCheckResult>,
    versionState: {} as Record<string, AsyncState>,
    versionErrors: {} as Record<string, string | null>,
    previews: {} as Record<string, AgentAssetReadResult>,
    previewState: {} as Record<string, AsyncState>,
    previewErrors: {} as Record<string, string | null>,
    inventoryRequestIds: {} as Record<string, number>,
    versionRequestIds: {} as Record<string, number>,
    previewRequestIds: {} as Record<string, number>,
  }),
  getters: {
    inventory: (state) => (workspace?: string) => {
      const key = agentEnvironmentKey(workspace);
      const inventory = state.inventories[key];
      return inventory ? mergeVersionFacts(inventory, state.versions[key]) : null;
    },
    inventoryLoading: (state) => (workspace?: string) => {
      const status = state.inventoryState[agentEnvironmentKey(workspace)];
      return status === "loading" || status === "refreshing";
    },
    installationById: (state) => (id: string, workspace?: string) => {
      const key = agentEnvironmentKey(workspace);
      const inventory = state.inventories[key];
      return inventory
        ? mergeVersionFacts(inventory, state.versions[key]).installations.find((item) => item.id === id) ?? null
        : null;
    },
  },
  actions: {
    async loadInventory(workspace?: string, forceRefresh = false) {
      const normalized = normalizedWorkspace(workspace);
      const key = agentEnvironmentKey(normalized);
      if (!forceRefresh && this.inventories[key] && this.inventoryState[key] === "ready") {
        return this.inventories[key];
      }
      const requestId = nextRequestId(this.inventoryRequestIds, key);
      const hadData = Boolean(this.inventories[key]);
      this.inventoryState[key] = hadData ? "refreshing" : "loading";
      this.inventoryErrors[key] = null;
      try {
        const result = await getAgentEnvironmentInventory(normalized);
        if (this.inventoryRequestIds[key] !== requestId) return result;
        this.inventories[key] = result;
        this.inventoryState[key] = "ready";
        return result;
      } catch (error) {
        if (this.inventoryRequestIds[key] !== requestId) throw error;
        this.inventoryState[key] = "error";
        this.inventoryErrors[key] = errorMessage(error);
        throw error;
      }
    },
    async refreshLatestVersions(workspace?: string) {
      const normalized = normalizedWorkspace(workspace);
      const key = agentEnvironmentKey(normalized);
      const requestId = nextRequestId(this.versionRequestIds, key);
      const hadData = Boolean(this.versions[key]);
      this.versionState[key] = hadData ? "refreshing" : "loading";
      this.versionErrors[key] = null;
      try {
        const result = await checkAgentLatestVersions(normalized);
        if (this.versionRequestIds[key] !== requestId) return result;
        this.versions[key] = result;
        this.versionState[key] = "ready";
        return result;
      } catch (error) {
        if (this.versionRequestIds[key] !== requestId) throw error;
        this.versionState[key] = "error";
        this.versionErrors[key] = errorMessage(error);
        throw error;
      }
    },
    async readConfigPreview(assetId: string, workspace?: string) {
      const normalized = normalizedWorkspace(workspace);
      const key = previewKey(assetId, normalized);
      const requestId = nextRequestId(this.previewRequestIds, key);
      this.previewState[key] = this.previews[key] ? "refreshing" : "loading";
      this.previewErrors[key] = null;
      try {
        const result = await readAgentEnvironmentAsset(assetId, normalized);
        if (this.previewRequestIds[key] !== requestId) return result;
        this.previews[key] = result;
        this.previewState[key] = "ready";
        return result;
      } catch (error) {
        if (this.previewRequestIds[key] !== requestId) throw error;
        this.previewState[key] = "error";
        this.previewErrors[key] = errorMessage(error);
        throw error;
      }
    },
    openAsset(assetId: string, workspace?: string, target: AgentAssetOpenTarget = "asset") {
      return openAgentEnvironmentAsset(assetId, normalizedWorkspace(workspace), target);
    },
    clearTransientState() {
      this.inventoryErrors = {};
      this.versionErrors = {};
      this.previewErrors = {};
    },
  },
});

export type AgentEnvironmentAsyncState = AsyncState;
export type { AgentInstallation };
