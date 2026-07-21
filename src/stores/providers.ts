import { defineStore } from "pinia";
import {
  activateTemporaryCli as activateTemporaryCliCommand,
  acknowledgeLivenessCost as acknowledgeLivenessCostCommand,
  revokeLivenessCost as revokeLivenessCostCommand,
  checkCliPath as checkCliPathCommand,
  browseWorkspaceDirectories as browseWorkspaceDirectoriesCommand,
  listCliCandidates as listCliCandidatesCommand,
  changeProviderPassword as changeProviderPasswordCommand,
  completeProviderCredentials as completeProviderCredentialsCommand,
  createProviderApiKey as createProviderApiKeyCommand,
  createProviderApiKeyForInput as createProviderApiKeyForInputCommand,
  deleteProviderApiKey as deleteProviderApiKeyCommand,
  exportAppData as exportAppDataCommand,
  generateProviderAccessTokenForInput as generateProviderAccessTokenForInputCommand,
  getCliRuntimeSnapshot as getCliRuntimeSnapshotCommand,
  getTemporaryCliInstances as getTemporaryCliInstancesCommand,
  getProviderCheckInRecords as getProviderCheckInRecordsCommand,
  getProviderInviteLink as getProviderInviteLinkCommand,
  getProviderRequestLogs as getProviderRequestLogsCommand,
  getProviderUsage as getProviderUsageCommand,
  forgetWorkspace as forgetWorkspaceCommand,
  importAppData as importAppDataCommand,
  launchTemporaryCli as launchTemporaryCliCommand,
  listProviderApiKeys as listProviderApiKeysCommand,
  loadAppData,
  probeCodexCli as probeCodexCliCommand,
  probeProviderSite as probeProviderSiteCommand,
  previewCliConfig as previewCliConfigCommand,
  refreshAllProviders,
  refreshProviders,
  removeProvider as removeProviderCommand,
  reorderProviders as reorderProvidersCommand,
  saveProvider as saveProviderCommand,
  saveSettings as saveSettingsCommand,
  syncCodexModels as syncCodexModelsCommand,
  probeProviderCapabilities as probeProviderCapabilitiesCommand,
  testLiveness as testLivenessCommand,
  testProviderConnection as testProviderConnectionCommand,
  switchCliConfig as switchCliConfigCommand,
} from "../api/app";
import type { CodexCliProbeInput } from "../api/app";
import { providerToInput } from "../utils/provider-input";
import { defaultSettings } from "./provider-defaults";
import type {
  AppSettings,
  CliConfigPreview,
  CliRuntimeSnapshot,
  LivenessCliKind,
  Provider,
  ProviderInput,
  ProviderRequestLogsQuery,
  TemporaryCliLaunchResult,
  TemporaryCliLaunchInput,
  TemporaryCliPreference,
  Workspace,
} from "./provider-types";

export { defaultSettings } from "./provider-defaults";
export type * from "./provider-types";

export const useProviderStore = defineStore("providers", {
  state: () => ({
    initialized: false,
    loading: false,
    loadError: null as string | null,
    refreshInProgress: false,
    cliRuntimeLoading: false,
    refreshingIds: new Set<string>(),
    providers: [] as Provider[],
    settings: defaultSettings(),
    workspaces: [] as Workspace[],
    temporaryCliPreferences: [] as TemporaryCliPreference[],
    cliRuntime: emptyCliRuntimeSnapshot(),
  }),
  getters: {},
  actions: {
    async initialize() {
      if (this.initialized || this.loading) {
        return;
      }

      this.loading = true;
      try {
        const data = await loadAppData();
        this.providers = data.providers;
        this.settings = data.settings;
        this.workspaces = data.workspaces;
        this.temporaryCliPreferences = data.temporaryCliPreferences;
        this.loadError = null;
        try {
          this.cliRuntime = await getCliRuntimeSnapshotCommand();
        } catch {
          this.cliRuntime = emptyCliRuntimeSnapshot();
        }
      } catch (error) {
        this.providers = [];
        this.loadError = errorToMessage(error);
      } finally {
        this.initialized = true;
        this.loading = false;
      }
    },
    async saveProvider(input: ProviderInput) {
      this.providers = await saveProviderCommand(input);
      await this.refreshCliRuntime().catch(() => {});
      return this.providers;
    },
    async removeProvider(id: string) {
      this.providers = await removeProviderCommand(id);
      this.temporaryCliPreferences = this.temporaryCliPreferences.filter(
        (preference) => preference.providerId !== id,
      );
      await this.refreshCliRuntime().catch(() => {});
    },
    async reorderProviders(ids: string[]) {
      this.providers = await reorderProvidersCommand(ids);
    },
    async toggleProvider(id: string, enabled: boolean) {
      const provider = this.providers.find((item) => item.identity.id === id);
      if (!provider) {
        return;
      }

      await this.saveProvider(providerToInput(provider, { runtime: { enabled } }));
    },
    async saveSettings(settings: AppSettings) {
      this.settings = await saveSettingsCommand(settings);
    },
    async exportAppData(path: string) {
      return exportAppDataCommand(path);
    },
    async importAppData(path: string) {
      const result = await importAppDataCommand(path);
      const data = await loadAppData();
      this.providers = data.providers;
      this.settings = data.settings;
      this.workspaces = data.workspaces;
      this.temporaryCliPreferences = data.temporaryCliPreferences;
      this.loadError = null;
      await this.refreshCliRuntime().catch(() => {});
      return result;
    },
    async reload() {
      try {
        const data = await loadAppData();
        this.providers = data.providers;
        this.settings = data.settings;
        this.workspaces = data.workspaces;
        this.temporaryCliPreferences = data.temporaryCliPreferences;
        this.loadError = null;
      } catch (error) {
        // 看板已有数据时，后台 tick 的一次瞬时失败不值得把整个界面切到全屏错误态；
        // 只有从未成功加载过才进入错误态。调用方可自行 catch 决定是否提示。
        if (this.providers.length === 0) {
          this.loadError = errorToMessage(error);
        }
        throw error;
      }
    },
    async probeProviderSite(input: ProviderInput) {
      return probeProviderSiteCommand(input);
    },
    async completeProviderCredentials(input: ProviderInput) {
      return completeProviderCredentialsCommand(input);
    },
    async testProviderConnection(input: ProviderInput) {
      const result = await testProviderConnectionCommand(input);
      if (input.id && result.ok) {
        await this.reload();
      }
      return result;
    },
    async probeCodexCli(input?: Partial<CodexCliProbeInput>) {
      const result = await probeCodexCliCommand(input);
      const data = await loadAppData();
      this.settings = data.settings;
      return result;
    },
    async checkCliPath(kind: LivenessCliKind, path: string) {
      return checkCliPathCommand(kind, path);
    },
    async listCliCandidates(kind: LivenessCliKind, path: string) {
      return listCliCandidatesCommand(kind, path);
    },
    async testLiveness(id: string, prompt?: string, automatic = false) {
      const result = await testLivenessCommand(id, prompt, automatic);
      this.providers = result.providers;
      return result;
    },
    async launchTemporaryCli(input: TemporaryCliLaunchInput): Promise<TemporaryCliLaunchResult> {
      const result = await launchTemporaryCliCommand(input);
      this.workspaces = result.workspaces;
      this.temporaryCliPreferences = [
        ...this.temporaryCliPreferences.filter(
          (preference) => preference.providerId !== result.preference.providerId,
        ),
        result.preference,
      ];
      await this.refreshCliRuntime().catch(() => {});
      return result;
    },
    async activateTemporaryCli(instanceId: string) {
      await activateTemporaryCliCommand(instanceId);
    },
    async refreshTemporaryCliInstances() {
      const instances = await getTemporaryCliInstancesCommand();
      this.cliRuntime = { ...this.cliRuntime, instances };
      return instances;
    },
    async browseWorkspaceDirectories(path?: string) {
      return browseWorkspaceDirectoriesCommand(path);
    },
    async forgetWorkspace(path: string) {
      this.workspaces = await forgetWorkspaceCommand(path);
      this.temporaryCliPreferences = this.temporaryCliPreferences.map((preference) =>
        preference.workspacePath === path ? { ...preference, workspacePath: "" } : preference,
      );
      return this.workspaces;
    },
    async previewCliConfig(id: string, cliKind: LivenessCliKind): Promise<CliConfigPreview> {
      return previewCliConfigCommand(id, cliKind);
    },
    async switchCliConfig(id: string, cliKind: LivenessCliKind, revision: string) {
      this.cliRuntime = await switchCliConfigCommand(id, cliKind, revision);
      return this.cliRuntime;
    },
    async refreshCliRuntime(): Promise<CliRuntimeSnapshot> {
      this.cliRuntimeLoading = true;
      try {
        this.cliRuntime = await getCliRuntimeSnapshotCommand();
        return this.cliRuntime;
      } finally {
        this.cliRuntimeLoading = false;
      }
    },
    async acknowledgeLivenessCost() {
      this.settings = await acknowledgeLivenessCostCommand();
      return this.settings;
    },
    async revokeLivenessCost() {
      this.settings = await revokeLivenessCostCommand();
      return this.settings;
    },
    async listApiKeys(id: string) {
      const options = await listProviderApiKeysCommand(id);
      await this.reload().catch(() => {});
      return options;
    },
    async createApiKey(id: string, name: string) {
      const options = await createProviderApiKeyCommand(id, name);
      await this.reload().catch(() => {});
      return options;
    },
    async createApiKeyForInput(input: ProviderInput, name: string) {
      return createProviderApiKeyForInputCommand(input, name);
    },
    async generateAccessTokenForInput(input: ProviderInput) {
      return generateProviderAccessTokenForInputCommand(input);
    },
    async deleteApiKey(id: string, tokenId: string) {
      const options = await deleteProviderApiKeyCommand(id, tokenId);
      await this.reload().catch(() => {});
      return options;
    },
    async getUsage(id: string, period = "24h") {
      return getProviderUsageCommand(id, period);
    },
    async getRequestLogs(id: string, query: ProviderRequestLogsQuery) {
      return getProviderRequestLogsCommand(id, query);
    },
    async changePassword(id: string, originalPassword: string, password: string) {
      return changeProviderPasswordCommand(id, originalPassword, password);
    },
    async getCheckInRecords(id: string, month: string) {
      return getProviderCheckInRecordsCommand(id, month);
    },
    async probeCapabilities(id: string) {
      const result = await probeProviderCapabilitiesCommand(id);
      this.providers = result.providers;
      return result;
    },
    async syncCodexModels(id: string) {
      const result = await syncCodexModelsCommand(id);
      this.providers = result.providers;
      return result;
    },
    async getInviteLink(id: string) {
      return getProviderInviteLinkCommand(id);
    },
    /** 全量刷新。返回错误信息（成功为 null），由调用方决定如何向用户呈现。 */
    async refreshAll(): Promise<string | null> {
      if (this.refreshInProgress) {
        return null;
      }

      this.refreshInProgress = true;
      const previousProviders = this.providers;
      this.providers = this.providers.map((provider) =>
        provider.runtime.enabled
          ? { ...provider, runtime: { ...provider.runtime, status: "syncing", errorMessage: null } }
          : provider,
      );

      try {
        const result = await refreshAllProviders();
        this.providers = result.providers;
        return null;
      } catch (error) {
        this.providers = previousProviders.map((provider) =>
          provider.runtime.enabled
            ? {
                ...provider,
                runtime: {
                  ...provider.runtime,
                  status: "error",
                  errorMessage: error instanceof Error ? error.message : String(error),
                },
              }
            : provider,
        );
        return errorToMessage(error);
      } finally {
        this.refreshInProgress = false;
      }
    },
    /** 按 id 刷新。返回错误信息（成功为 null），由调用方决定如何向用户呈现。 */
    async refreshByIds(ids: string[]): Promise<string | null> {
      const todo = ids.filter((id) => !this.refreshingIds.has(id));
      if (todo.length === 0) {
        return null;
      }

      todo.forEach((id) => this.refreshingIds.add(id));
      const idSet = new Set(todo);
      const previousProviders = this.providers;
      this.providers = this.providers.map((provider) =>
        provider.runtime.enabled && idSet.has(provider.identity.id)
          ? { ...provider, runtime: { ...provider.runtime, status: "syncing", errorMessage: null } }
          : provider,
      );

      try {
        const result = await refreshProviders(todo);
        this.providers = result.providers;
        return null;
      } catch (error) {
        this.providers = previousProviders.map((provider) =>
          provider.runtime.enabled && idSet.has(provider.identity.id)
            ? {
                ...provider,
                runtime: {
                  ...provider.runtime,
                  status: "error",
                  errorMessage: error instanceof Error ? error.message : String(error),
                },
              }
            : provider,
        );
        return errorToMessage(error);
      } finally {
        todo.forEach((id) => this.refreshingIds.delete(id));
      }
    },
  },
});

function errorToMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function emptyCliRuntimeSnapshot(): CliRuntimeSnapshot {
  const emptyConfig = () => ({
    configured: false,
    providerId: null,
    modifiedAt: null,
    errorMessage: null,
  });
  return {
    codex: emptyConfig(),
    claudeCode: emptyConfig(),
    instances: [],
  };
}
