<script setup lang="ts">
import { onScopeDispose, ref, watch } from "vue";
import { ChevronLeft } from "@lucide/vue";
import { Message } from "@arco-design/web-vue";
import type { Provider } from "../../stores/provider-types";
import type { CpaPlugin, CpaPluginList } from "../../stores/oauth-types";
import {
  getCpaPluginConfig,
  listCpaPlugins,
  removeCpaPlugin,
  setCpaPluginEnabled,
  updateCpaPluginConfig,
} from "../../api/oauth-accounts";
import { confirmAction } from "../../composables/provider-credential-dialogs";
import ContentEditor from "../ContentEditor.vue";
import CpaPluginStore from "./CpaPluginStore.vue";

const props = defineProps<{
  visible: boolean;
  provider: Provider;
  revision: number;
}>();
const emit = defineEmits<{ close: [] }>();

/// Reading the store makes the gateway call every configured source, so it is
/// only done after the user asks for it.
const view = ref<"installed" | "store">("installed");

const plugins = ref<CpaPluginList | null>(null);
const loading = ref(false);
const error = ref("");
/// The plugin whose switch is being written; the row keeps its old state until
/// the gateway confirms.
const toggling = ref<string | null>(null);
/// The plugin whose removal is running; the row keeps its position until the
/// gateway confirms, and every other row stays usable.
const removing = ref<string | null>(null);
/// The plugin whose configuration is open, with its text kept apart from the
/// saved value so 放弃 restores exactly what the gateway returned.
const editing = ref<CpaPlugin | null>(null);
const configText = ref("");
const configOriginal = ref("");
const configLoading = ref(false);
const configSaving = ref(false);
const configError = ref("");
let pluginRequest = 0;
/// Removals are guarded apart from reads: the list read that follows a
/// successful removal must not make the finishing removal look stale.
let removeRequest = 0;
let disposed = false;
onScopeDispose(() => {
  disposed = true;
  pluginRequest++;
  removeRequest++;
});
watch(
  /// Only the connection and the open state reset the dialog: a background
  /// card refresh must not close a configuration the user is still editing.
  () => [props.visible, props.provider.identity.id],
  ([visible]) => {
    pluginRequest++;
    removeRequest++;
    plugins.value = null;
    error.value = "";
    loading.value = false;
    toggling.value = null;
    removing.value = null;
    view.value = "installed";
    closeConfig();
    if (visible) void load();
  },
  { immediate: true },
);
async function load() {
  const sequence = ++pluginRequest;
  const id = props.provider.identity.id;
  loading.value = true;
  error.value = "";
  try {
    const result = await listCpaPlugins(id);
    if (!disposed && sequence === pluginRequest) plugins.value = result;
  } catch (cause) {
    if (!disposed && sequence === pluginRequest) error.value = String(cause);
  } finally {
    if (sequence === pluginRequest) loading.value = false;
  }
}
async function toggle(plugin: CpaPlugin, value: boolean) {
  if (!plugins.value || toggling.value) return;
  const id = props.provider.identity.id;
  toggling.value = plugin.id;
  error.value = "";
  try {
    await setCpaPluginEnabled(id, plugin.id, value, props.revision);
    if (disposed || !plugins.value) return;
    plugin.enabled = value;
    /// The gateway folds the per-plugin switch, the system switch and
    /// registration into one effective flag, so the tags move with it.
    plugin.effectiveEnabled =
      plugins.value.enabled && value && plugin.registered;
    Message.success(value ? "插件已启用" : "插件已停用");
  } catch (cause) {
    if (!disposed) error.value = String(cause);
  } finally {
    toggling.value = null;
  }
}
async function openConfig(plugin: CpaPlugin) {
  /// Switching to another plugin while a read is in flight is allowed; the
  /// sequence number drops the answer that no longer matches the open row.
  if (editing.value?.id === plugin.id && configLoading.value) return;
  const sequence = ++pluginRequest;
  const id = props.provider.identity.id;
  editing.value = plugin;
  configText.value = "";
  configOriginal.value = "";
  configError.value = "";
  configLoading.value = true;
  try {
    const result = await getCpaPluginConfig(id, plugin.id);
    if (disposed || sequence !== pluginRequest) return;
    configText.value = result.content;
    configOriginal.value = result.content;
  } catch (cause) {
    if (!disposed && sequence === pluginRequest) configError.value = String(cause);
  } finally {
    if (sequence === pluginRequest) configLoading.value = false;
  }
}
function closeConfig() {
  pluginRequest++;
  editing.value = null;
  configText.value = "";
  configOriginal.value = "";
  configError.value = "";
  configLoading.value = false;
}
const configDirty = () => configText.value !== configOriginal.value;
/// Removing deletes the plugin file together with `plugins.configs.<id>`, so
/// the credentials the plugin stored are gone as well. A plugin that is still
/// loaded has to be unloaded by a gateway restart, which the gateway reports.
async function remove(plugin: CpaPlugin) {
  if (removing.value) return;
  const confirmed = await confirmAction(
    `移除插件 · ${plugin.name || plugin.id}`,
    `网关会删除该插件的文件和 plugins.configs.${plugin.id}，插件自己保存的凭据一并删除。正在运行的插件需要重启网关后才能移除。`,
    "移除插件",
    "danger",
  );
  if (!confirmed || disposed) return;
  const sequence = ++removeRequest;
  const id = props.provider.identity.id;
  removing.value = plugin.id;
  error.value = "";
  try {
    const result = await removeCpaPlugin(id, plugin.id, props.revision);
    if (disposed || sequence !== removeRequest) return;
    Message.success(
      result.configuredRemoved ? "插件与配置已移除" : "插件文件已移除",
    );
  } catch (cause) {
    if (!disposed && sequence === removeRequest) error.value = String(cause);
  } finally {
    if (sequence === removeRequest) removing.value = null;
  }
  /// A removed plugin is gone from both the gateway list and the store's
  /// installed state, so the list is read again instead of patched locally.
  if (!disposed && sequence === removeRequest) await load();
}
async function saveConfig() {
  const plugin = editing.value;
  if (!plugin || configSaving.value) return;
  const id = props.provider.identity.id;
  configSaving.value = true;
  configError.value = "";
  try {
    const result = await updateCpaPluginConfig(
      id,
      plugin.id,
      configText.value,
      props.revision,
    );
    if (disposed || editing.value !== plugin) return;
    /// The gateway may normalize the object it stored; keep the confirmed
    /// version instead of the draft so the next save starts from it.
    configText.value = result.content;
    configOriginal.value = result.content;
    plugin.configured = true;
    Message.success("插件配置已保存");
  } catch (cause) {
    if (!disposed) configError.value = String(cause);
  } finally {
    configSaving.value = false;
  }
}
</script>
<template>
  <a-modal
    :visible="visible"
    :title="editing ? `插件配置 · ${editing.name}` : 'CPA 插件'"
    width="680px"
    :footer="false"
    unmount-on-close
    @cancel="emit('close')"
  >
    <template v-if="editing">
      <div class="cpa-plugin-editor-head">
        <a-button size="mini" @click="closeConfig"
          ><template #icon><ChevronLeft :size="12" /></template>返回插件列表</a-button
        >
      </div>
      <a-alert v-if="configError" type="warning">{{ configError }}</a-alert>
      <a-spin v-if="configLoading" tip="读取插件配置" />
      <template v-else>
        <div class="cpa-plugin-editor">
          <ContentEditor
            :model-value="configText"
            :original-text="configOriginal"
            format="json"
            :label="`${editing.name} 配置`"
            @update:model-value="configText = $event"
          />
        </div>
        <p class="cpa-plugin-hint">
          插件的配置保存为网关 `plugins.configs.{{ editing.id }}`；保存后网关会重新加载配置，正在处理的请求不受影响。
        </p>
        <div class="cpa-plugin-savebar">
          <a-button
            size="mini"
            :disabled="!configDirty() || configSaving"
            @click="configText = configOriginal"
            >放弃修改</a-button
          ><a-button
            size="mini"
            type="primary"
            :disabled="!configDirty()"
            :loading="configSaving"
            @click="saveConfig"
            >保存配置</a-button
          >
        </div>
      </template>
    </template>
    <template v-else>
      <div class="cpa-plugin-views">
        <a-radio-group v-model="view" type="button" size="mini">
          <a-radio value="installed">已安装</a-radio>
          <a-radio value="store">插件商店</a-radio>
        </a-radio-group>
      </div>
      <CpaPluginStore
        v-if="view === 'store'"
        :provider="provider"
        :revision="revision"
        @installed="load"
      />
      <template v-else>
        <a-alert v-if="error" type="warning">{{ error }}</a-alert>
        <a-spin v-if="loading" tip="读取 CPA 插件" />
        <template v-else-if="plugins">
          <a-alert v-if="!plugins.enabled" type="info"
            >网关未开启插件功能：以下为已发现但不会加载的插件，开关只有在网关开启插件后才能生效。</a-alert
          >
          <div v-if="plugins.plugins.length" class="cpa-plugin-list">
            <div
              v-for="plugin in plugins.plugins"
              :key="plugin.id"
              class="cpa-plugin-row"
            >
              <div class="cpa-plugin-copy">
                <strong>{{ plugin.name }}</strong
                ><span
                  >{{ plugin.id }}<template v-if="plugin.version">
                    · v{{ plugin.version }}</template
                  ></span
                >
                <div class="cpa-plugin-tags">
                  <span v-if="plugin.effectiveEnabled" class="is-enabled"
                    >已启用</span
                  ><span v-else-if="plugin.enabled" class="is-off"
                    >已开启未加载</span
                  ><span v-else class="is-off">未启用</span
                  ><span v-if="plugin.configured">已配置</span
                  ><span v-if="plugin.registered">已注册</span
                  ><span v-if="plugin.supportsQuota">额度</span
                  ><span v-if="plugin.oauthProvider">授权入口</span>
                </div>
              </div>
              <div class="cpa-plugin-actions">
                <a-button
                  size="mini"
                  :disabled="toggling !== null || removing !== null"
                  @click="openConfig(plugin)"
                  >配置</a-button
                ><a-button
                  size="mini"
                  status="danger"
                  :disabled="toggling !== null || removing !== null"
                  :loading="removing === plugin.id"
                  @click="remove(plugin)"
                  >移除</a-button
                ><a-switch
                  size="small"
                  :model-value="plugin.enabled"
                  :disabled="!plugins.enabled || toggling !== null || removing !== null"
                  :loading="toggling === plugin.id"
                  :aria-label="`启用 ${plugin.name}`"
                  @change="toggle(plugin, !!$event)"
                />
              </div>
            </div>
          </div>
          <p v-else class="cpa-plugin-empty">CPA 未报告已安装的插件</p>
        </template>
      </template>
    </template>
  </a-modal>
</template>
<style scoped>
.cpa-plugin-views {
  margin-bottom: 10px;
}
.cpa-plugin-list {
  display: grid;
  gap: 6px;
}
.cpa-plugin-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 14px;
  padding: 10px 12px;
  border: 1px solid var(--color-border-2);
  border-radius: 10px;
  background: var(--color-fill-1);
}
.cpa-plugin-copy {
  display: grid;
  min-width: 0;
  gap: 2px;
}
.cpa-plugin-copy strong {
  overflow: hidden;
  color: var(--color-text-1);
  font-size: 13px;
  font-weight: 500;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.cpa-plugin-copy > span {
  overflow: hidden;
  color: var(--color-text-3);
  font-size: 11px;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.cpa-plugin-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  margin-top: 3px;
}
.cpa-plugin-tags span {
  border: 1px solid var(--color-border-2);
  border-radius: 999px;
  color: var(--color-text-3);
  font-size: 10px;
  line-height: 16px;
  padding: 0 7px;
}
.cpa-plugin-tags .is-enabled {
  border-color: color-mix(in srgb, rgb(var(--green-6)) 30%, transparent);
  background: color-mix(in srgb, rgb(var(--green-6)) 9%, transparent);
  color: rgb(var(--green-6));
}
.cpa-plugin-tags .is-off {
  background: var(--color-fill-2);
}
.cpa-plugin-actions {
  display: flex;
  flex-shrink: 0;
  align-items: center;
  justify-content: flex-end;
  gap: 8px;
}
.cpa-plugin-savebar {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 10px;
}
.cpa-plugin-editor-head {
  margin-bottom: 10px;
}
.cpa-plugin-editor {
  display: flex;
  flex-direction: column;
  height: calc(100vh - 260px);
  min-height: 300px;
}
.cpa-plugin-editor :deep(.content-editor) {
  flex: 1;
  min-height: 0;
}
.cpa-plugin-hint {
  margin: 10px 0 0;
  color: var(--color-text-3);
  font-size: 11px;
  line-height: 1.6;
}
.cpa-plugin-empty {
  margin: 0;
  color: var(--color-text-3);
  font-size: 12px;
  line-height: 1.65;
}
</style>
