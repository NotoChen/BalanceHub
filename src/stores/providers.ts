import { defineStore } from "pinia";
import {
  acknowledgeLivenessCost as acknowledgeLivenessCostCommand,
  revokeLivenessCost as revokeLivenessCostCommand,
  checkCliPath as checkCliPathCommand,
  listCliCandidates as listCliCandidatesCommand,
  changeProviderPassword as changeProviderPasswordCommand,
  completeProviderCredentials as completeProviderCredentialsCommand,
  createProviderApiKey as createProviderApiKeyCommand,
  createProviderApiKeyForInput as createProviderApiKeyForInputCommand,
  deleteProviderApiKey as deleteProviderApiKeyCommand,
  exportAppData as exportAppDataCommand,
  generateProviderAccessToken as generateProviderAccessTokenCommand,
  generateProviderAccessTokenForInput as generateProviderAccessTokenForInputCommand,
  getProviderCheckInRecords as getProviderCheckInRecordsCommand,
  getProviderInviteLink as getProviderInviteLinkCommand,
  getProviderRequestLogs as getProviderRequestLogsCommand,
  getProviderUsage as getProviderUsageCommand,
  importAppData as importAppDataCommand,
  listProviderApiKeys as listProviderApiKeysCommand,
  loadAppData,
  previewLivenessCommand as previewLivenessCommandCommand,
  probeCodexCli as probeCodexCliCommand,
  probeProviderSite as probeProviderSiteCommand,
  refreshAllProviders,
  refreshProviders,
  removeProvider as removeProviderCommand,
  reorderProviders as reorderProvidersCommand,
  saveProvider as saveProviderCommand,
  saveSettings as saveSettingsCommand,
  syncCodexModels as syncCodexModelsCommand,
  syncProviderCapabilities as syncProviderCapabilitiesCommand,
  testLiveness as testLivenessCommand,
  testProviderConnection as testProviderConnectionCommand,
} from "../api/app";
import type { CodexCliProbeInput } from "../api/app";
import { providerToInput } from "../utils/provider-input";
import { defaultSettings } from "./provider-defaults";
import type {
  AppSettings,
  LivenessCliKind,
  Provider,
  ProviderInput,
  ProviderRequestLogsQuery,
} from "./provider-types";

export { defaultSettings } from "./provider-defaults";
export type * from "./provider-types";

export const useProviderStore = defineStore("providers", {
  state: () => ({
    initialized: false,
    loading: false,
    loadError: null as string | null,
    refreshInProgress: false,
    refreshingIds: new Set<string>(),
    providers: [] as Provider[],
    settings: defaultSettings(),
  }),
  getters: {
    activeProviders: (state) => state.providers.filter((provider) => provider.runtime.enabled),
    failedProviders: (state) =>
      state.providers.filter((provider) => provider.runtime.enabled && provider.runtime.status === "error"),
    totalAvailable: (state) =>
      state.providers
        .filter((provider) => provider.runtime.enabled && !provider.quota.unlimited)
        .reduce((total, provider) => total + provider.quota.available, 0),
    totalUsed: (state) =>
      state.providers
        .filter((provider) => provider.runtime.enabled)
        .reduce((total, provider) => total + provider.quota.used, 0),
    lastSyncedAt: (state) => {
      const syncedValues = state.providers
        .map((provider) => provider.automation.lastSyncedAt)
        .filter((value): value is string => Boolean(value))
        .sort();

      return syncedValues.length === 0 ? null : syncedValues[syncedValues.length - 1];
    },
  },
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
        this.loadError = null;
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
      return this.providers;
    },
    async removeProvider(id: string) {
      this.providers = await removeProviderCommand(id);
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
      this.loadError = null;
      return result;
    },
    async reload() {
      try {
        const data = await loadAppData();
        this.providers = data.providers;
        this.settings = data.settings;
        this.loadError = null;
      } catch (error) {
        this.loadError = errorToMessage(error);
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
    async previewLivenessCommand(id: string) {
      return previewLivenessCommandCommand(id);
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
    async acknowledgeLivenessCost() {
      this.settings = await acknowledgeLivenessCostCommand();
      return this.settings;
    },
    async revokeLivenessCost() {
      this.settings = await revokeLivenessCostCommand();
      return this.settings;
    },
    async listApiKeys(id: string) {
      return listProviderApiKeysCommand(id);
    },
    async createApiKey(id: string, name: string) {
      return createProviderApiKeyCommand(id, name);
    },
    async createApiKeyForInput(input: ProviderInput, name: string) {
      return createProviderApiKeyForInputCommand(input, name);
    },
    async generateAccessToken(id: string) {
      this.providers = await generateProviderAccessTokenCommand(id);
      return this.providers.find((provider) => provider.identity.id === id) ?? null;
    },
    async generateAccessTokenForInput(input: ProviderInput) {
      return generateProviderAccessTokenForInputCommand(input);
    },
    async deleteApiKey(id: string, tokenId: string) {
      return deleteProviderApiKeyCommand(id, tokenId);
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
    async syncCapabilities(id: string) {
      const result = await syncProviderCapabilitiesCommand(id);
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
    async refreshAll() {
      if (this.refreshInProgress) {
        return;
      }

      this.refreshInProgress = true;
      const previousProviders = this.providers;
      this.providers = this.providers.map((provider) =>
        provider.runtime.enabled ? { ...provider, status: "syncing", errorMessage: null } : provider,
      );

      try {
        const result = await refreshAllProviders();
        this.providers = result.providers;
      } catch (error) {
        this.providers = previousProviders.map((provider) =>
          provider.runtime.enabled
            ? {
                ...provider,
                status: "error",
                errorMessage: error instanceof Error ? error.message : String(error),
              }
            : provider,
        );
      } finally {
        this.refreshInProgress = false;
      }
    },
    async refreshByIds(ids: string[]) {
      const todo = ids.filter((id) => !this.refreshingIds.has(id));
      if (todo.length === 0) {
        return;
      }

      todo.forEach((id) => this.refreshingIds.add(id));
      const idSet = new Set(todo);
      const previousProviders = this.providers;
      this.providers = this.providers.map((provider) =>
        provider.runtime.enabled && idSet.has(provider.identity.id)
          ? { ...provider, status: "syncing", errorMessage: null }
          : provider,
      );

      try {
        const result = await refreshProviders(todo);
        this.providers = result.providers;
      } catch (error) {
        this.providers = previousProviders.map((provider) =>
          provider.runtime.enabled && idSet.has(provider.identity.id)
            ? {
                ...provider,
                status: "error",
                errorMessage: error instanceof Error ? error.message : String(error),
              }
            : provider,
        );
      } finally {
        todo.forEach((id) => this.refreshingIds.delete(id));
      }
    },
  },
});

function errorToMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
