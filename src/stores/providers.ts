import { defineStore } from "pinia";
import { toRaw } from "vue";
import {
  changeProviderPassword as changeProviderPasswordCommand,
  changeProviderProfile as changeProviderProfileCommand,
  redeemProviderCode as redeemProviderCodeCommand,
  completeProviderCredentials as completeProviderCredentialsCommand,
  createProviderApiKey as createProviderApiKeyCommand,
  updateProviderApiKey as updateProviderApiKeyCommand,
  getProviderApiKeyEditorContext,
  getProviderApiKeyEditorContextForInput,
  createProviderApiKeyForInput as createProviderApiKeyForInputCommand,
  deleteProviderApiKey as deleteProviderApiKeyCommand,
  detectProviderProtocol as detectProviderProtocolCommand,
  exportAppData as exportAppDataCommand,
  generateProviderAccessTokenForInput as generateProviderAccessTokenForInputCommand,
  getProviderCheckInRecords as getProviderCheckInRecordsCommand,
  getProviderInviteLink as getProviderInviteLinkCommand,
  getProviderRequestLogs as getProviderRequestLogsCommand,
  getProviderUsage as getProviderUsageCommand,
  importAppData as importAppDataCommand,
  installDevSampleData,
  addLocalProviderApiKey as addLocalProviderApiKeyCommand,
  listLocalProviderApiKeys as listLocalProviderApiKeysCommand,
  listProviderApiKeys as listProviderApiKeysCommand,
  loadAppData,
  loadProviderViews,
  probeProviderSite as probeProviderSiteCommand,
  previewProviderCheckInPolicy as previewProviderCheckInPolicyCommand,
  refreshProviders,
  removeProvider as removeProviderCommand,
  reorderProviders as reorderProvidersCommand,
  removeLocalProviderApiKey as removeLocalProviderApiKeyCommand,
  restoreLatestBackup as restoreLatestBackupCommand,
  setLocalProviderApiKeyRemark as setLocalProviderApiKeyRemarkCommand,
  saveProvider as saveProviderCommand,
  setProviderEnabled,
  setProviderAgentModel as setProviderAgentModelCommand,
  setDefaultLocalProviderApiKey as setDefaultLocalProviderApiKeyCommand,
  syncAvailableModels as syncAvailableModelsCommand,
  probeProviderCapabilities as probeProviderCapabilitiesCommand,
  testProviderConnection as testProviderConnectionCommand,
  type AppData,
  type AppDataImportResult,
} from "../api/app";
import { withTimeout } from "../utils/promise-timeout";
import {
  mergeProvidersByRevision,
  pruneProviderTombstones,
  sameActiveEndpoint,
  type ProviderRevisionTombstones,
} from "../utils/provider-revision";
import { useCliRuntimeStore } from "./cli-runtime";
import { useSettingsStore } from "./settings";
import { useWorkspaceStore } from "./workspaces";
import type {
  AgentCliKind,
  Provider,
  ProviderInput,
  ProviderApiKeyPatch,
  ProviderProtocolDescriptor,
  ProviderSaveOptions,
  ProviderRequestLogsQuery,
} from "./provider-types";

export { defaultSettings } from "./provider-defaults";
export type * from "./provider-types";

/// 一次完整快照读取当时在飞的去向。后台一轮批量变更（例如逐个站点完成的
/// 签到）会连发多条 providers-changed；同一时刻只允许一个快照在飞，期间
/// 到达的请求合并成一次补拉，避免 WebView 反复解析、替换整份看板数据。
const pendingProviderReloads = new WeakMap<object, Promise<void>>();

export const useProviderStore = defineStore("providers", {
  state: () => ({
    initialized: false,
    loading: false,
    loadError: null as string | null,
    /// 运行期进入存储保护态（例如云同步应用失败）：保存已暂停，只能重启恢复。
    storageBlockedError: null as string | null,
    refreshInProgress: false,
    refreshingIds: new Set<string>(),
    providerReloadPending: false,
    providers: [] as Provider[],
    providerProtocols: [] as ProviderProtocolDescriptor[],
    providerSnapshotRevision: 0,
    providerOrderRequestId: 0,
    providerTombstones: {} as ProviderRevisionTombstones,
  }),
  getters: {},
  actions: {
    previewCheckInPolicy(input: ProviderInput) {
      return previewProviderCheckInPolicyCommand(input);
    },
    replaceProviders(providers: Provider[]) {
      this.providers = providers;
    },
    replaceProviderSnapshot(providers: Provider[], revision: number) {
      // 定向读取可能已经把个别站点推进到比上次完整快照更新的版本。比它更旧
      // 的完整快照既不能覆盖这些结果，也不能据此判定刚新增的站点已被删除。
      const newestKnown = this.providers.reduce(
        (newest, provider) => Math.max(newest, provider.revision ?? 0),
        this.providerSnapshotRevision,
      );
      if (revision < newestKnown) return false;
      // 后端每次快照都会重新序列化整份数据；内容没变的中转站复用旧对象，
      // 卡片才能跳过重渲染（对象身份是 Vue 子组件更新的边界）。revision 是
      // 进程内的全局版本戳，每次持久化都会推进到同一值，比较前先归一化，
      // 否则它会把「内容其实没变」全部掩盖掉。
      //
      // 版本号没变说明上次快照之后没有任何持久化：整份内容相同，只有进程内
      // 的备用地址读数可能变化。这时逐个站点比这一个字段即可，不再为整份
      // 快照做两遍全量序列化。
      const sameRevision = revision === this.providerSnapshotRevision;
      const previous = new Map(
        this.providers.map((provider) => [provider.identity.id, toRaw(provider)] as const),
      );
      const next = providers.map((provider) => {
        const current = previous.get(provider.identity.id);
        if (!current) return provider;
        const unchanged = sameRevision
          ? sameActiveEndpoint(current, provider)
          : JSON.stringify({ ...current, revision: 0 })
            === JSON.stringify({ ...provider, revision: 0 });
        if (!unchanged) return provider;
        // 旧对象要带上本轮快照的版本号，后续按 revision 合并时才不会把
        // 晚到的旧响应误判成更新。
        current.revision = provider.revision;
        return current;
      });
      // 整份快照一块都没变时连数组也不换：重连、外部配置无实质变化这类
      // 补拉不会再触发整块看板重渲染。
      if (
        next.length !== this.providers.length
        || next.some((provider, index) => provider !== toRaw(this.providers[index]))
      ) {
        this.providers = next;
      }
      this.providerSnapshotRevision = revision;
      this.providerTombstones = pruneProviderTombstones(this.providerTombstones, revision);
      return true;
    },
    hydrateAppData(data: AppData) {
      if (!this.replaceProviderSnapshot(data.providers, data.revision)) {
        return false;
      }
      this.providerProtocols = data.providerProtocols;
      useSettingsStore().hydrate(data.settings);
      useWorkspaceStore().hydrate(data.workspaces, data.temporaryCliPreferences);
      this.loadError = null;
      this.storageBlockedError = null;
      return true;
    },
    markStorageBlocked(error: string) {
      this.storageBlockedError = error;
    },
    upsertProvider(provider: Provider) {
      this.upsertProviders([provider]);
    },
    upsertProviders(providers: Provider[]) {
      this.providers = mergeProvidersByRevision(
        this.providers,
        providers,
        this.providerSnapshotRevision,
        this.providerTombstones,
      );
    },
    removeProviderById(id: string) {
      this.providers = this.providers.filter((provider) => provider.identity.id !== id);
    },
    applyProviderOrder(ids: string[]) {
      const providers = new Map(
        this.providers.map((provider) => [provider.identity.id, provider] as const),
      );
      const ordered = ids.flatMap((id) => {
        const provider = providers.get(id);
        if (!provider) return [];
        providers.delete(id);
        return [provider];
      });
      this.providers = [...ordered, ...providers.values()];
    },
    async initialize() {
      if ((this.initialized && !this.loadError) || this.loading) {
        return;
      }

      this.loading = true;
      try {
        const data = await withTimeout(loadAppData(), 15_000, "读取本地配置超时，请重试");
        this.hydrateAppData(data);
        void useCliRuntimeStore().refresh().catch(() => {});
      } catch (error) {
        this.providers = [];
        this.providerSnapshotRevision = 0;
        this.providerTombstones = {};
        this.loadError = errorToMessage(error);
      } finally {
        this.initialized = true;
        this.loading = false;
      }
    },
    async saveProvider(input: ProviderInput, options: ProviderSaveOptions = {}) {
      const result = await saveProviderCommand(input, options);
      if (result.saved && result.provider) {
        this.upsertProvider(result.provider);
        // Runtime discovery is independent of persistence and must not delay its acknowledgement.
        void useCliRuntimeStore().refresh().catch(() => {});
      }
      return result;
    },
    async removeProvider(id: string) {
      const result = await withTimeout(
        removeProviderCommand(id),
        15_000,
        "删除中转站超时，请刷新列表核对",
      );
      this.providerTombstones[result.id] = Math.max(
        this.providerTombstones[result.id] ?? 0,
        result.revision,
      );
      this.removeProviderById(result.id);
      useWorkspaceStore().removeProviderPreference(result.id);
      await useCliRuntimeStore().refresh().catch(() => {});
    },
    async reorderProviders(ids: string[]) {
      const requestId = ++this.providerOrderRequestId;
      const order = await withTimeout(reorderProvidersCommand(ids), 15_000, "保存中转站排序超时，请重试");
      if (requestId === this.providerOrderRequestId) this.applyProviderOrder(order);
    },
    async toggleProvider(id: string, enabled: boolean) {
      const provider = this.providers.find((item) => item.identity.id === id);
      if (!provider) {
        return;
      }

      this.upsertProvider(await withTimeout(setProviderEnabled(id, enabled), 15_000, "更新卡片状态超时，请刷新后核对"));
    },
    async setAgentModel(id: string, cliKind: AgentCliKind, model: string) {
      this.upsertProvider(await withTimeout(
        setProviderAgentModelCommand(id, cliKind, model.trim() || null),
        15_000,
        "保存 Agent 模型绑定超时，请刷新后核对",
      ));
    },
    async exportAppData(path: string) {
      return exportAppDataCommand(path);
    },
    async importAppData(path: string) {
      return this.applyImportedData(() => importAppDataCommand(path));
    },
    /** 启动读取失败后，用同目录最近的自动备份原地恢复。 */
    async restoreLatestBackup() {
      return this.applyImportedData(() => restoreLatestBackupCommand());
    },
    /** 开发版演示数据与导入走同一条水合路径，避免两种写法各自维护状态合并。 */
    async installDevSampleData() {
      const providerCount = await installDevSampleData();
      await this.applyImportedData(async () => ({
        data: await loadAppData(),
        transfer: { path: "", schemaVersion: 0, providerCount },
      }));
    },
    async applyImportedData(request: () => Promise<AppDataImportResult>) {
      const result = await request();
      if (!this.hydrateAppData(result.data)) {
        // 另一个后端事务可能在导入命令返回到 WebView 前已经产生了更新版本。
        // 只接受再次读取到的最新完整快照，绝不把旧 settings/workspaces 灌回前端。
        const latest = await loadAppData();
        if (!this.hydrateAppData(latest)) {
          this.providerReloadPending = true;
        }
      }
      void useCliRuntimeStore().refresh().catch(() => {});
      return result.transfer;
    },
    async reload() {
      if (this.refreshInProgress || this.refreshingIds.size > 0) {
        this.providerReloadPending = true;
        return;
      }
      const inFlight = pendingProviderReloads.get(this);
      if (inFlight) {
        // 本轮读取开始得比这次请求早；等它连同随后的补拉一起完成再返回，
        // 调用方拿到的快照不会落后于自己刚看到的变更。
        this.providerReloadPending = true;
        await inFlight.catch(() => {});
        return;
      }
      const run = (async () => {
        do {
          this.providerReloadPending = false;
          const data = await loadAppData();
          this.hydrateAppData(data);
        } while (
          this.providerReloadPending
          && !this.refreshInProgress
          && this.refreshingIds.size === 0
        );
        if (this.providerReloadPending) {
          // 期间进入了批量/手动刷新：补拉留给它们结束后的 flush。
          void this.flushPendingProviderReload();
        }
      })();
      pendingProviderReloads.set(this, run);
      try {
        await run;
      } catch (error) {
        // 看板已有数据时，后台 tick 的一次瞬时失败不值得把整个界面切到全屏错误态；
        // 只有从未成功加载过才进入错误态。调用方可自行 catch 决定是否提示。
        if (this.providers.length === 0) {
          this.loadError = errorToMessage(error);
        }
        throw error;
      } finally {
        pendingProviderReloads.delete(this);
      }
    },
    /**
     * Refresh the view after a background Rust mutation. Do not replace the
     * local syncing state while a provider operation is still in flight; the
     * operation returns the authoritative provider view when it completes.
     */
    async reloadProviders() {
      await this.reload();
    },
    async flushPendingProviderReload() {
      if (
        !this.providerReloadPending ||
        this.refreshInProgress || this.refreshingIds.size > 0
      ) {
        return;
      }
      this.providerReloadPending = false;
      await this.reload().catch(() => {});
    },
    async reloadProviderViews(ids: string[]) {
      // 单卡或一组卡片动作后只补这些站点的最新视图；邀请链接、Key 列表、
      // 签到完成这类操作不必再让后端序列化、让 WebView 解析整份配置。
      const unique = [...new Set(ids)];
      if (unique.length === 0) return [];
      const data = await loadProviderViews(unique);
      // 一次合并整组视图：逐张调用会反复复制整个 providers 数组，
      // 批量签到完成时这个成本随站点数量平方增长。
      if (data.providers.length > 0) this.upsertProviders(data.providers);
      const returned = new Set(data.providers.map((provider) => provider.identity.id));
      // 只有这份响应不早于最近一次完整快照时，缺失才能证明站点已删除；
      // 否则可能把完整快照里刚恢复的站点误删。
      if (data.revision >= this.providerSnapshotRevision) {
        const missing = unique.filter((id) => !returned.has(id));
        if (missing.length > 0) {
          const removed = new Set(missing);
          for (const id of missing) {
            this.providerTombstones[id] = Math.max(
              this.providerTombstones[id] ?? 0,
              data.revision,
            );
          }
          this.providers = this.providers.filter(
            (provider) => !removed.has(provider.identity.id),
          );
        }
      }
      return data.providers;
    },
    async reloadProvider(id: string) {
      const [provider] = await this.reloadProviderViews([id]);
      return provider ?? null;
    },
    async probeProviderSite(input: ProviderInput) {
      return probeProviderSiteCommand(input);
    },
    async detectProviderProtocol(input: ProviderInput) {
      return detectProviderProtocolCommand(input);
    },
    async completeProviderCredentials(input: ProviderInput) {
      return completeProviderCredentialsCommand(input);
    },
    async testProviderConnection(input: ProviderInput) {
      const result = await testProviderConnectionCommand(input);
      if (input.id && result.ok) {
        await this.reloadProvider(input.id);
      }
      return result;
    },
    async listApiKeys(id: string) {
      const options = await listProviderApiKeysCommand(id);
      await this.reloadProvider(id).catch(() => {});
      return options;
    },
    async listLocalApiKeys(id: string) {
      return listLocalProviderApiKeysCommand(id);
    },
    async addLocalApiKey(id: string, key: string, remark: string) {
      const provider = await addLocalProviderApiKeyCommand(id, key, remark);
      this.upsertProvider(provider);
      return provider;
    },
    async setLocalApiKeyRemark(id: string, localId: string, remark: string) {
      const provider = await setLocalProviderApiKeyRemarkCommand(id, localId, remark);
      this.upsertProvider(provider);
      return provider;
    },
    async setDefaultLocalApiKey(id: string, localId: string) {
      const provider = await setDefaultLocalProviderApiKeyCommand(id, localId);
      this.upsertProvider(provider);
      return provider;
    },
    async removeLocalApiKey(id: string, localId: string) {
      const provider = await removeLocalProviderApiKeyCommand(id, localId);
      this.upsertProvider(provider);
      return provider;
    },
    apiKeyEditorContext(id: string, tokenId: string | null = null) {
      return getProviderApiKeyEditorContext(id, tokenId);
    },
    apiKeyEditorContextForInput(input: ProviderInput) {
      return getProviderApiKeyEditorContextForInput(input);
    },
    async updateApiKey(id: string, credentialRevision: number, tokenId: string, patch: ProviderApiKeyPatch) {
      const options = await updateProviderApiKeyCommand(id, credentialRevision, tokenId, patch);
      await this.reloadProvider(id).catch(() => {});
      return options;
    },
    async createApiKey(id: string, credentialRevision: number, patch: ProviderApiKeyPatch) {
      const options = await createProviderApiKeyCommand(id, credentialRevision, patch);
      await this.reloadProvider(id).catch(() => {});
      return options;
    },
    async createApiKeyForInput(input: ProviderInput, patch: ProviderApiKeyPatch) {
      return createProviderApiKeyForInputCommand(input, patch);
    },
    async generateAccessTokenForInput(input: ProviderInput) {
      return generateProviderAccessTokenForInputCommand(input);
    },
    async deleteApiKey(id: string, tokenId: string) {
      const options = await deleteProviderApiKeyCommand(id, tokenId);
      await this.reloadProvider(id).catch(() => {});
      return options;
    },
    async getUsage(id: string, period = "24h") {
      return getProviderUsageCommand(id, period);
    },
    async getRequestLogs(id: string, query: ProviderRequestLogsQuery) {
      return getProviderRequestLogsCommand(id, query);
    },
    async changePassword(id: string, originalPassword: string, password: string) {
      const message = await changeProviderPasswordCommand(id, originalPassword, password);
      // 用户认证模式下后端会同步本地 loginPassword；重新读取让编辑器拿到最新凭据，
      // 避免用户随后保存编辑表单时把旧密码写回去。
      await this.reloadProvider(id).catch(() => {});
      return message;
    },
    async changeProfile(id: string, username: string, displayName: string) {
      const result = await changeProviderProfileCommand(id, username, displayName);
      this.upsertProvider(result.provider);
      return result;
    },
    async redeemCode(id: string, code: string) {
      const result = await redeemProviderCodeCommand(id, code);
      this.upsertProvider(result.provider);
      return result;
    },
    async getCheckInRecords(id: string, month: string) {
      return getProviderCheckInRecordsCommand(id, month);
    },
    async probeCapabilities(id: string) {
      const result = await withTimeout(
        probeProviderCapabilitiesCommand(id),
        120_000,
        "站点能力探测超时，请稍后重试",
      );
      this.upsertProvider(result.provider);
      return result;
    },
    async syncAvailableModels(id: string) {
      const result = await syncAvailableModelsCommand(id);
      this.upsertProvider(result.provider);
      return result;
    },
    async getInviteLink(id: string) {
      const link = await withTimeout(
        getProviderInviteLinkCommand(id),
        30_000,
        "读取邀请链接超时，请重试",
      );
      await this.reloadProvider(id).catch(() => {});
      return link;
    },
    /** 按 id 刷新。返回错误信息（成功为 null），由调用方决定如何向用户呈现。 */
    async refreshByIds(ids: string[]): Promise<string | null> {
      if (this.refreshInProgress) {
        // 全局刷新本身就覆盖所有卡片；单卡刷新不再排第二个批次，
        // 标记一次重读，让这轮结果写回后再刷新界面。
        this.providerReloadPending = true;
        return null;
      }
      const todo = ids.filter((id) => !this.refreshingIds.has(id));
      if (todo.length === 0) {
        return null;
      }

      todo.forEach((id) => this.refreshingIds.add(id));
      const idSet = new Set(todo);
      this.providers = this.providers.map((provider) =>
        provider.runtime.enabled && idSet.has(provider.identity.id)
          ? { ...provider, runtime: { ...provider.runtime, status: "syncing", errorMessage: null } }
          : provider,
      );

      const request = refreshProviders(todo);
      try {
        const result = await withTimeout(
          request,
          180_000,
          "刷新等待超时，后台完成后会自动更新",
        );
        this.upsertProviders(result.updatedProviders);
        return null;
      } catch (error) {
        // 前端等待窗口到期不等于后端放弃：刷新可能只是在排队等自动刷新
        // 释放闸门，请求仍会在后台完成。完成后再重读一次，避免卡片一直
        // 停在「刷新超时」而真实结果其实早已写回。
        void request
          .then(() => {
            this.providerReloadPending = true;
            void this.flushPendingProviderReload();
          })
          .catch(() => {});
        this.providers = this.providers.map((provider) =>
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
        void this.flushPendingProviderReload();
      }
    },
  },
});

function errorToMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
