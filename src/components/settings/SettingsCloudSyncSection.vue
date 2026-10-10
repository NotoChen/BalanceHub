<script setup lang="ts">
import { computed, inject, reactive, ref, watch } from "vue";
import { Modal } from "@arco-design/web-vue";
import { Cloud, RefreshCw, Settings2, ShieldCheck } from "@lucide/vue";
import { CLOUD_SYNC_CONTEXT } from "../../composables/useCloudSync";
import type {
  CloudSyncSettings,
  CloudSyncSettingsInput,
} from "../../api/cloud-sync";
import { formatProgress } from "../../utils/progress-display";
import ContextDetails from "../ContextDetails.vue";

const sync = inject(CLOUD_SYNC_CONTEXT);
const state = computed(() => sync?.state.value);
const status = computed(() => state.value?.status);
const configured = computed(() =>
  Boolean(
    state.value?.settings.serverUrl && state.value.settings.hasPassphrase,
  ),
);
const busy = computed(() =>
  Boolean(sync?.pending.value || status.value?.running),
);
const editing = ref(false);
const dirty = ref(false);
const passwordEdited = ref(false);
const passphraseEdited = ref(false);
const draft = reactive({
  serverUrl: "",
  username: "",
  password: "",
  passphrase: "",
  remoteRoot: "BalanceHub",
  deviceName: "这台电脑",
});
let loaded = false;
let draftRevision = 0;

function edited() {
  dirty.value = true;
  draftRevision++;
}

function fill(settings: CloudSyncSettings) {
  Object.assign(draft, {
    serverUrl: settings.serverUrl,
    username: settings.username,
    remoteRoot: settings.remoteRoot,
    deviceName: settings.deviceName,
    password: "",
    passphrase: "",
  });
  passwordEdited.value = false;
  passphraseEdited.value = false;
  dirty.value = false;
}
watch(
  () => state.value?.settings,
  (settings) => {
    if (!settings) return;
    if (!dirty.value) fill(settings);
    if (!loaded) {
      editing.value = !configured.value;
      loaded = true;
    }
  },
  { immediate: true },
);

function input(): CloudSyncSettingsInput {
  return {
    ...draft,
    password: passwordEdited.value && draft.password ? draft.password : null,
    passphrase:
      passphraseEdited.value && draft.passphrase ? draft.passphrase : null,
    autoSync: state.value?.settings.autoSync ?? false,
  };
}
async function submit(action: "test" | "sync") {
  if (!sync || busy.value) return;
  const submittedRevision = draftRevision;
  if (!(await sync.save(input()))) return;
  if (submittedRevision === draftRevision) {
    if (state.value) fill(state.value.settings);
    editing.value = action === "test" || !configured.value;
  } else {
    editing.value = true;
  }
  if (action === "test") await sync.test();
  else await sync.sync();
}
function openEditor() {
  if (state.value && !dirty.value) fill(state.value.settings);
  editing.value = true;
}
function cancelEdit() {
  if (state.value) fill(state.value.settings);
  editing.value = false;
}
async function toggleAuto(value: string | number | boolean) {
  const saved = state.value?.settings;
  if (!saved || !sync) return;
  await sync.save({
    ...saved,
    password: null,
    passphrase: null,
    autoSync: Boolean(value),
  });
}
function restore() {
  Modal.confirm({
    title: "恢复同步前的本机配置",
    content:
      "将恢复上次应用云端变更前的中转站、偏好与共享库。自动同步会暂停，云端数据保持当前版本；本机 Agent 的原生配置不受影响。",
    okText: "恢复本机配置",
    cancelText: "取消",
    onOk: () => {
      void sync?.restore();
    },
  });
}
const date = (value: number) =>
  new Date(value).toLocaleString("zh-CN", { hour12: false });
</script>

<template>
  <section
    v-if="sync"
    class="settings-card cloud-sync-settings"
    aria-label="WebDAV 同步"
  >
    <header class="settings-card-header cloud-sync-heading">
      <span class="settings-card-icon"><Cloud :size="18" /></span>
      <div><strong>WebDAV 同步</strong></div>
      <label class="cloud-sync-auto"
        ><span>自动同步</span
        ><a-switch
          :model-value="state?.settings.autoSync ?? false"
          :disabled="!configured || busy || dirty"
          aria-label="自动 WebDAV 同步"
          @change="toggleAuto"
      /></label>
    </header>
    <div class="cloud-sync-body">
      <p class="cloud-sync-description">
        在多台设备间同步中转站、应用偏好与共享资产。
      </p>
      <div v-if="!state" class="cloud-sync-state">
        <span>{{ sync.error.value || "正在读取同步设置…" }}</span
        ><a-button size="small" @click="sync.refresh">重试</a-button>
      </div>

      <div
        v-if="state && status?.message"
        class="cloud-sync-state"
        :class="{
          'is-error': status?.phase === 'failed',
          'is-review': status?.review,
        }"
        role="status"
        aria-live="polite"
      >
        <span
          ><strong>{{ status?.message || "已保存连接，可立即同步" }}</strong
          ><small v-if="status?.lastSyncedAt"
            >上次同步 {{ date(status.lastSyncedAt) }}</small
          ><small v-if="status?.retryAt"
            >将在 {{ date(status.retryAt) }} 后重试</small
          ></span
        >
        <a-button v-if="status?.review" type="primary" @click="sync.openReview"
          >查看差异</a-button
        >
      </div>
      <a-progress
        v-if="status?.running && status.progress !== null"
        :percent="status.progress"
        ><template #text="{ percent }">{{
          formatProgress(percent)
        }}</template></a-progress
      >
      <div v-if="state && !editing" class="cloud-sync-overview">
        <div class="cloud-sync-endpoint" :title="state.settings.serverUrl">
          {{ state.settings.serverUrl
          }}<span
            >{{ state.settings.remoteRoot }} ·
            {{ state.settings.deviceName }}</span
          >
        </div>
        <div class="cloud-sync-actions">
          <a-button
            v-if="!status?.running && !status?.review"
            type="primary"
            :loading="sync.pending.value === 'sync'"
            :disabled="busy"
            @click="sync.sync"
            ><template #icon><RefreshCw :size="14" /></template
            >立即同步</a-button
          >
          <a-button
            v-if="status?.canCancel"
            :disabled="Boolean(sync.pending.value)"
            @click="sync.cancel"
            >取消本次同步</a-button
          >
          <a-button :disabled="busy" @click="openEditor"
            ><template #icon><Settings2 :size="14" /></template
            >连接设置</a-button
          >
          <span class="cloud-sync-encryption"
            ><ShieldCheck :size="14" />上传前加密</span
          >
        </div>
      </div>

      <div v-if="state && editing" class="cloud-sync-form">
        <div class="cloud-sync-field cloud-sync-field-wide">
          <label for="cloud-sync-url">WebDAV 服务地址</label
          ><a-input
            v-model="draft.serverUrl"
            placeholder="https://dav.example.com/"
            :input-attrs="{
              id: 'cloud-sync-url',
              'aria-label': 'WebDAV 服务地址',
              autocomplete: 'url',
              spellcheck: false,
            }"
            @input="edited"
          />
        </div>
        <div class="cloud-sync-field">
          <label for="cloud-sync-user">账号</label
          ><a-input
            v-model="draft.username"
            placeholder="WebDAV 用户名"
            :input-attrs="{
              id: 'cloud-sync-user',
              'aria-label': 'WebDAV 账号',
              autocomplete: 'off',
            }"
            @input="edited"
          />
        </div>
        <div class="cloud-sync-field">
          <label for="cloud-sync-password">应用密码</label
          ><a-input-password
            v-model="draft.password"
            :placeholder="
              state.settings.hasPassword
                ? '已保存，留空保留'
                : 'WebDAV 登录密码或应用密码'
            "
            :input-attrs="{
              id: 'cloud-sync-password',
              'aria-label': 'WebDAV 应用密码',
              autocomplete: 'new-password',
            }"
            @input="
              edited();
              passwordEdited = true;
            "
          />
        </div>
        <div class="cloud-sync-field cloud-sync-field-wide">
          <label for="cloud-sync-passphrase">同步密码</label
          ><a-input-password
            v-model="draft.passphrase"
            :placeholder="
              state.settings.hasPassphrase
                ? '已保存，留空保留'
                : '至少 12 个字符，用于加密同步数据'
            "
            :input-attrs="{
              id: 'cloud-sync-passphrase',
              'aria-label': '同步密码',
              autocomplete: 'new-password',
            }"
            @input="
              edited();
              passphraseEdited = true;
            "
          /><small
            >所有设备使用同一个同步密码，请自行妥善保存；遗失后无法解密云端数据。</small
          >
        </div>
        <div class="cloud-sync-field">
          <label for="cloud-sync-directory">同步目录</label
          ><a-input
            v-model="draft.remoteRoot"
            placeholder="BalanceHub"
            :input-attrs="{
              id: 'cloud-sync-directory',
              'aria-label': '同步目录',
            }"
            @input="edited"
          />
        </div>
        <div class="cloud-sync-field">
          <label for="cloud-sync-device">设备名称</label
          ><a-input
            v-model="draft.deviceName"
            placeholder="例如：办公电脑"
            :max-length="80"
            :input-attrs="{ id: 'cloud-sync-device', 'aria-label': '设备名称' }"
            @input="edited"
          />
        </div>
        <div class="cloud-sync-actions cloud-sync-field-wide">
          <a-button
            type="primary"
            :loading="
              sync.pending.value === 'save' || sync.pending.value === 'sync'
            "
            :disabled="busy || !draft.serverUrl"
            @click="submit('sync')"
            >保存并同步</a-button
          >
          <a-button
            :disabled="busy || !draft.serverUrl"
            :loading="sync.pending.value === 'test'"
            @click="submit('test')"
            >测试连接</a-button
          >
          <a-button
            v-if="status?.canCancel"
            :disabled="Boolean(sync.pending.value)"
            @click="sync.cancel"
            >取消本次同步</a-button
          >
          <a-button
            v-if="configured"
            :disabled="Boolean(sync.pending.value)"
            @click="cancelEdit"
            >取消编辑</a-button
          >
          <span v-if="dirty" class="cloud-sync-draft-note"
            >连接设置尚未保存</span
          >
        </div>
      </div>
      <a-alert v-if="sync.error.value && state" type="error">{{
        sync.error.value
      }}</a-alert>
      <ContextDetails label="同步范围与恢复">
        <p>
          同步中转站基础配置、认证输入、API Key
          库、排序、应用偏好，以及共享库中的 Skill、MCP、Hook。Skill
          资源按文件增量传输。
        </p>
        <p>
          代理、工作目录、浏览器会话、Agent
          本机配置和运行记录留在各自设备。自动签到、刷新与测活开关也由每台设备单独控制。
        </p>
        <p>
          启用自动同步后，保存配置会触发同步，并每分钟检查云端变化。首次连接和相同配置的冲突需要确认；删除会传播到其他设备的同步数据。
        </p>
        <p>
          服务需支持文件读写、强 ETag
          和条件写入，测试连接会实际验证。连接使用应用的网络代理，建议填写 HTTPS
          地址。
        </p>
        <a-button
          v-if="state?.hasRecovery"
          size="small"
          :disabled="busy"
          @click="restore"
          >恢复同步前的本机配置</a-button
        >
      </ContextDetails>
    </div>
  </section>
</template>

<style scoped>
.cloud-sync-body {
  display: grid;
  gap: 16px;
  padding: 18px;
}
.cloud-sync-heading {
  margin-bottom: 0;
}
.cloud-sync-auto {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-left: auto;
  font-size: 12px;
  color: var(--color-text-2);
}
.cloud-sync-description {
  margin: 0;
  color: var(--color-text-3);
  font-size: 13px;
}
.cloud-sync-overview {
  display: grid;
  gap: 14px;
}
.cloud-sync-state {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  min-width: 0;
  font-size: 13px;
}
.cloud-sync-state > span {
  display: grid;
  gap: 7px;
  overflow-wrap: anywhere;
}
.cloud-sync-state strong {
  font-weight: 500;
  line-height: 1.6;
}
.cloud-sync-state small {
  font-size: 12px;
  color: var(--color-text-3);
}
.cloud-sync-state.is-error strong {
  color: rgb(var(--danger-6));
}
.cloud-sync-state.is-review strong {
  color: rgb(var(--orange-6));
}
.cloud-sync-endpoint {
  display: grid;
  gap: 5px;
  font-size: 12px;
  color: var(--color-text-3);
  overflow-wrap: anywhere;
}
.cloud-sync-actions {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 10px;
}
.cloud-sync-encryption {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  margin-left: auto;
  color: var(--color-text-3);
  font-size: 12px;
}
.cloud-sync-form {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 18px 20px;
}
.cloud-sync-field {
  display: grid;
  gap: 8px;
  align-content: start;
  min-width: 0;
}
.cloud-sync-field > label {
  font-size: 13px;
  color: var(--color-text-1);
}
.cloud-sync-field-wide {
  grid-column: 1 / -1;
}
.cloud-sync-field small,
.cloud-sync-draft-note {
  font-size: 12px;
  line-height: 1.6;
  color: var(--color-text-3);
}
@media (max-width: 680px) {
  .cloud-sync-form {
    grid-template-columns: 1fr;
  }
  .cloud-sync-auto {
    gap: 6px;
  }
}
</style>
