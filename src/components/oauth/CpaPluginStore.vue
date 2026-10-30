<script setup lang="ts">
import { computed, onScopeDispose, ref, watch } from "vue";
import { Message } from "@arco-design/web-vue";
import type { Provider } from "../../stores/provider-types";
import type { CpaPluginStore, CpaStorePlugin } from "../../stores/oauth-types";
import {
  installCpaPlugin,
  listCpaPluginStore,
} from "../../api/oauth-accounts";
import { confirmAction } from "../../composables/provider-credential-dialogs";

const props = defineProps<{
  provider: Provider;
  revision: number;
}>();
const emit = defineEmits<{ installed: [] }>();

const store = ref<CpaPluginStore | null>(null);
const loading = ref(false);
const error = ref("");
const notice = ref("");
const filter = ref("");
/// The entry being installed, keyed by source and id, so only that row shows
/// the gateway work and every other row stays clickable.
const installing = ref("");
let request = 0;
/// Installs are guarded apart from list reads: the read that follows a
/// successful install must not make the finishing install look stale.
let installRequest = 0;
let disposed = false;
onScopeDispose(() => {
  disposed = true;
  request++;
  installRequest++;
});
watch(
  () => props.provider.identity.id,
  () => {
    request++;
    installRequest++;
    store.value = null;
    error.value = "";
    notice.value = "";
    filter.value = "";
    installing.value = "";
    loading.value = false;
    void load();
  },
  { immediate: true },
);

async function load() {
  if (loading.value) return;
  const sequence = ++request;
  const id = props.provider.identity.id;
  loading.value = true;
  error.value = "";
  try {
    const result = await listCpaPluginStore(id);
    if (disposed || sequence !== request) return;
    store.value = result;
  } catch (cause) {
    if (!disposed && sequence === request) error.value = String(cause);
  } finally {
    if (sequence === request) loading.value = false;
  }
}

function key(entry: CpaStorePlugin) {
  return `${entry.sourceId}/${entry.id}`;
}
const entries = computed(() => {
  const text = filter.value.trim().toLowerCase();
  const plugins = store.value?.plugins ?? [];
  if (!text) return plugins;
  return plugins.filter((entry) =>
    [entry.id, entry.name, entry.description, entry.author, ...entry.tags]
      .join(" ")
      .toLowerCase()
      .includes(text),
  );
});

/// Installing fetches a release from the source and runs it on the gateway, so
/// the confirm names the release, the source and what the plugin will be able
/// to do. The gateway itself decides whether a restart is needed.
async function install(entry: CpaStorePlugin) {
  if (installing.value) return;
  const verb = entry.updateAvailable ? "更新" : "安装";
  const current = entry.installedVersion
    ? `（当前 v${entry.installedVersion}）`
    : "";
  /// `authRequired` is about the download source, not the plugin: the gateway
  /// has to hold matching `plugins.store.auth` or the install cannot start.
  const needsSourceAuth = entry.authRequired && !entry.authConfigured;
  const trusted = `网关会从该来源下载插件文件并运行它。第三方插件由作者维护，请确认来源可信。${
    needsSourceAuth
      ? "该来源需要授权，网关尚未配置匹配的 plugins.store.auth，安装会失败。"
      : ""
  }`;
  const confirmed = await confirmAction(
    `${verb}插件 · ${entry.name || entry.id}`,
    `${entry.author ? `作者 ${entry.author}，` : ""}版本 v${entry.version}${current}，来源 ${
      entry.sourceName || entry.sourceId
    }${entry.repository ? `，仓库 ${entry.repository}` : ""}。${trusted}`,
    `${verb}插件`,
    "warning",
  );
  if (!confirmed || disposed) return;
  const sequence = ++installRequest;
  const id = props.provider.identity.id;
  const entryKey = key(entry);
  installing.value = entryKey;
  error.value = "";
  notice.value = "";
  try {
    const result = await installCpaPlugin(
      id,
      entry.id,
      entry.sourceId,
      entry.version,
      props.revision,
    );
    if (disposed || sequence !== installRequest) return;
    notice.value = result.restartRequired
      ? `已安装 ${entry.name || result.id} v${result.version}，重启网关后生效`
      : `已安装 ${entry.name || result.id} v${result.version}`;
    Message.success(notice.value);
    emit("installed");
  } catch (cause) {
    if (!disposed && sequence === installRequest) error.value = String(cause);
  } finally {
    if (sequence === installRequest) installing.value = "";
  }
  /// The entry's own row now describes the copy on the gateway, and a plugin
  /// the gateway just wrote is only discoverable through a fresh read.
  if (!disposed && sequence === installRequest) await load();
}

function sourceFailure(error: { sourceName: string; sourceId: string; message: string }) {
  return `${error.sourceName || error.sourceId || "插件来源"} 读取失败：${
    error.message || "来源没有返回可用的索引"
  }`;
}
</script>
<template>
  <div class="cpa-store">
    <div class="cpa-store-head">
      <a-input
        v-model="filter"
        size="small"
        placeholder="搜索插件名称、作者或标签"
        allow-clear
      />
      <a-button size="mini" :loading="loading" @click="load">刷新</a-button>
    </div>
    <a-alert v-if="error" type="warning">{{ error }}</a-alert>
    <a-alert v-if="notice" type="success">{{ notice }}</a-alert>
    <a-spin v-if="loading" tip="读取插件商店" />
    <template v-else-if="store">
      <a-alert v-if="!store.enabled" type="info"
        >网关未开启插件功能：安装的插件会在网关开启后才加载。</a-alert
      >
      <a-alert
        v-for="failure in store.sourceErrors"
        :key="`${failure.sourceId}/${failure.message}`"
        type="warning"
        >{{ sourceFailure(failure) }}</a-alert
      >
      <div v-if="entries.length" class="cpa-store-list">
        <div
          v-for="entry in entries"
          :key="key(entry)"
          class="cpa-store-row"
        >
          <div class="cpa-store-copy">
            <strong
              >{{ entry.name || entry.id
              }}<span class="cpa-store-version"
                >v{{ entry.version }}</span
              ></strong
            >
            <span class="cpa-store-meta"
              ><template v-if="entry.author">{{ entry.author }} · </template
              >{{ entry.sourceName || entry.sourceId
              }}<template v-if="entry.repository">
                · {{ entry.repository }}</template
              ></span
            >
            <span v-if="entry.description" class="cpa-store-desc">{{
              entry.description
            }}</span>
            <div class="cpa-store-tags">
              <span v-if="entry.installed"
                >已安装<template v-if="entry.installedVersion">
                  v{{ entry.installedVersion }}</template
                ></span
              ><span v-if="entry.updateAvailable" class="is-update">可更新</span
              ><span
                v-if="entry.authRequired && !entry.authConfigured"
                class="is-need"
                >来源缺少授权</span
              ><span v-for="tag in entry.tags" :key="tag">{{ tag }}</span>
            </div>
          </div>
          <a-button
            size="mini"
            :type="entry.installed ? 'secondary' : 'primary'"
            :disabled="!!installing && installing !== key(entry)"
            :loading="installing === key(entry)"
            @click="install(entry)"
            >{{
              entry.updateAvailable
                ? "更新"
                : entry.installed
                  ? "重新安装"
                  : "安装"
            }}</a-button
          >
        </div>
      </div>
      <p v-else class="cpa-store-empty">
        {{
          filter.trim()
            ? "没有匹配的插件"
            : "网关的插件商店没有提供条目；可在网关配置的 plugins.store.sources 里添加来源。"
        }}
      </p>
    </template>
  </div>
</template>
<style scoped>
.cpa-store-head {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 10px;
}
.cpa-store-head :deep(.arco-input-wrapper) {
  flex: 1;
}
.cpa-store-list {
  display: grid;
  gap: 6px;
}
.cpa-store-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 14px;
  padding: 10px 12px;
  border: 1px solid var(--color-border-2);
  border-radius: 10px;
  background: var(--color-fill-1);
}
.cpa-store-copy {
  display: grid;
  min-width: 0;
  gap: 2px;
}
.cpa-store-copy strong {
  display: flex;
  align-items: baseline;
  gap: 6px;
  min-width: 0;
  color: var(--color-text-1);
  font-size: 13px;
  font-weight: 500;
}
.cpa-store-version {
  color: var(--color-text-3);
  font-size: 11px;
  font-weight: 400;
}
.cpa-store-meta {
  overflow: hidden;
  color: var(--color-text-3);
  font-size: 11px;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.cpa-store-desc {
  color: var(--color-text-2);
  font-size: 12px;
  line-height: 1.55;
}
.cpa-store-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  margin-top: 3px;
}
.cpa-store-tags span {
  border: 1px solid var(--color-border-2);
  border-radius: 999px;
  color: var(--color-text-3);
  font-size: 10px;
  line-height: 16px;
  padding: 0 7px;
}
.cpa-store-tags .is-update {
  border-color: color-mix(in srgb, rgb(var(--arcoblue-6)) 30%, transparent);
  background: color-mix(in srgb, rgb(var(--arcoblue-6)) 9%, transparent);
  color: rgb(var(--arcoblue-6));
}
.cpa-store-tags .is-need {
  border-color: color-mix(in srgb, rgb(var(--orange-6)) 30%, transparent);
  background: color-mix(in srgb, rgb(var(--orange-6)) 9%, transparent);
  color: rgb(var(--orange-6));
}
.cpa-store-empty {
  margin: 0;
  color: var(--color-text-3);
  font-size: 12px;
  line-height: 1.65;
}
</style>
