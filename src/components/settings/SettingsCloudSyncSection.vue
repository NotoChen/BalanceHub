<script setup lang="ts">
import { computed, inject, reactive, ref, watch } from "vue";
import { Modal } from "@arco-design/web-vue";
import { Cloud, RefreshCw, Settings2, ShieldCheck } from "@lucide/vue";
import { CLOUD_SYNC_CONTEXT } from "../../composables/useCloudSync";
import type {
  CloudSyncBackend,
  CloudSyncSettings,
  CloudSyncSettingsInput,
} from "../../api/cloud-sync";
import { formatProgress } from "../../utils/progress-display";
import ContextDetails from "../ContextDetails.vue";

const sync = inject(CLOUD_SYNC_CONTEXT);
const state = computed(() => sync?.state.value);
const status = computed(() => state.value?.status);
const backendOptions = [
  { label: "WebDAV", value: "webdav" },
  { label: "S3 兼容存储", value: "s3" },
  { label: "GitHub 仓库", value: "github" },
];
const endpointLabel = computed(() => {
  const settings = state.value?.settings;
  if (!settings) return "";
  if (settings.backend === "s3") {
    return settings.s3Endpoint
      ? `S3 · ${settings.s3Bucket} · ${settings.s3Endpoint}`
      : `S3 · ${settings.s3Bucket}`;
  }
  if (settings.backend === "github") {
    return `GitHub · ${settings.githubOwner}/${settings.githubRepo} · ${settings.githubBranch}`;
  }
  return settings.serverUrl;
});
const configured = computed(() => {
  const settings = state.value?.settings;
  if (!settings?.hasPassphrase) return false;
  if (settings.backend === "s3") {
    return Boolean(
      settings.s3Bucket && settings.s3AccessKey && settings.hasSecretKey,
    );
  }
  if (settings.backend === "github") {
    return Boolean(
      settings.githubOwner && settings.githubRepo && settings.hasGithubToken,
    );
  }
  return Boolean(settings.serverUrl);
});
const busy = computed(() =>
  Boolean(sync?.pending.value || status.value?.running),
);
const editing = ref(false);
const dirty = ref(false);
const passwordEdited = ref(false);
const githubTokenEdited = ref(false);
const secretEdited = ref(false);
const passphraseEdited = ref(false);
const draft = reactive({
  backend: "webdav" as CloudSyncBackend,
  serverUrl: "",
  username: "",
  password: "",
  passphrase: "",
  remoteRoot: "BalanceHub",
  githubOwner: "",
  githubRepo: "",
  githubBranch: "main",
  githubToken: "",
  s3Endpoint: "",
  s3Region: "us-east-1",
  s3Bucket: "",
  s3AccessKey: "",
  s3SecretKey: "",
  deviceName: "这台电脑",
});
const draftReady = computed(() => {
  if (draft.backend === "s3") {
    return Boolean(
      draft.s3Bucket &&
        draft.s3AccessKey &&
        (draft.s3SecretKey || state.value?.settings.hasSecretKey),
    );
  }
  if (draft.backend === "github") {
    return Boolean(
      draft.githubOwner &&
        draft.githubRepo &&
        draft.githubBranch &&
        (draft.githubToken || state.value?.settings.hasGithubToken),
    );
  }
  return Boolean(draft.serverUrl);
});
let loaded = false;
let draftRevision = 0;

function edited() {
  dirty.value = true;
  draftRevision++;
}

function fill(settings: CloudSyncSettings) {
  Object.assign(draft, {
    backend: settings.backend,
    serverUrl: settings.serverUrl,
    username: settings.username,
    remoteRoot: settings.remoteRoot,
    githubOwner: settings.githubOwner,
    githubRepo: settings.githubRepo,
    githubBranch: settings.githubBranch,
    s3Endpoint: settings.s3Endpoint,
    s3Region: settings.s3Region,
    s3Bucket: settings.s3Bucket,
    s3AccessKey: settings.s3AccessKey,
    deviceName: settings.deviceName,
    password: "",
    githubToken: "",
    s3SecretKey: "",
    passphrase: "",
  });
  passwordEdited.value = false;
  githubTokenEdited.value = false;
  secretEdited.value = false;
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
    backend: draft.backend,
    serverUrl: draft.serverUrl,
    username: draft.username,
    password: passwordEdited.value && draft.password ? draft.password : null,
    passphrase:
      passphraseEdited.value && draft.passphrase ? draft.passphrase : null,
    remoteRoot: draft.remoteRoot,
    githubOwner: draft.githubOwner,
    githubRepo: draft.githubRepo,
    githubBranch: draft.githubBranch,
    githubToken:
      githubTokenEdited.value && draft.githubToken ? draft.githubToken : null,
    s3Endpoint: draft.s3Endpoint,
    s3Region: draft.s3Region,
    s3Bucket: draft.s3Bucket,
    s3AccessKey: draft.s3AccessKey,
    s3SecretKey:
      secretEdited.value && draft.s3SecretKey ? draft.s3SecretKey : null,
    deviceName: draft.deviceName,
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
    githubToken: null,
    s3SecretKey: null,
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
    aria-label="云端同步"
  >
    <header class="settings-card-header cloud-sync-heading">
      <span class="settings-card-icon"><Cloud :size="18" /></span>
      <div><strong>云端同步</strong></div>
      <label class="cloud-sync-auto"
        ><span>自动同步</span
        ><a-switch
          :model-value="state?.settings.autoSync ?? false"
          :disabled="!configured || busy || dirty"
          aria-label="自动云端同步"
          @change="toggleAuto"
      /></label>
    </header>
    <div class="cloud-sync-body">
      <p class="cloud-sync-description">
        在多台设备间同步中转站、应用偏好与共享资产，支持 WebDAV、S3
        兼容存储与 GitHub 仓库。
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
        <div class="cloud-sync-endpoint" :title="endpointLabel">
          {{ endpointLabel }}<span
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
          <span class="cloud-sync-field-label">存储类型</span>
          <a-radio-group
            v-model="draft.backend"
            type="button"
            :options="backendOptions"
            aria-label="云端同步存储类型"
            @change="edited"
          />
        </div>
        <template v-if="draft.backend === 'webdav'">
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
        </template>
        <template v-else-if="draft.backend === 's3'">
          <div class="cloud-sync-field cloud-sync-field-wide">
            <label for="cloud-sync-s3-endpoint">服务地址</label
            ><a-input
              v-model="draft.s3Endpoint"
              placeholder="https://s3.us-east-1.amazonaws.com（留空按区域生成）"
              :input-attrs="{
                id: 'cloud-sync-s3-endpoint',
                'aria-label': 'S3 服务地址',
                autocomplete: 'url',
                spellcheck: false,
              }"
              @input="edited"
            /><small
              >AWS S3 可留空；Cloudflare R2、MinIO、Backblaze B2
              等填写各自的服务地址。</small
            >
          </div>
          <div class="cloud-sync-field">
            <label for="cloud-sync-s3-bucket">存储桶</label
            ><a-input
              v-model="draft.s3Bucket"
              placeholder="balancehub"
              :input-attrs="{
                id: 'cloud-sync-s3-bucket',
                'aria-label': 'S3 存储桶',
                autocomplete: 'off',
                spellcheck: false,
              }"
              @input="edited"
            />
          </div>
          <div class="cloud-sync-field">
            <label for="cloud-sync-s3-region">区域</label
            ><a-input
              v-model="draft.s3Region"
              placeholder="us-east-1"
              :input-attrs="{
                id: 'cloud-sync-s3-region',
                'aria-label': 'S3 区域',
                autocomplete: 'off',
                spellcheck: false,
              }"
              @input="edited"
            />
          </div>
          <div class="cloud-sync-field">
            <label for="cloud-sync-s3-access">Access Key</label
            ><a-input
              v-model="draft.s3AccessKey"
              placeholder="S3 Access Key ID"
              :input-attrs="{
                id: 'cloud-sync-s3-access',
                'aria-label': 'S3 Access Key',
                autocomplete: 'off',
                spellcheck: false,
              }"
              @input="edited"
            />
          </div>
          <div class="cloud-sync-field">
            <label for="cloud-sync-s3-secret">Secret Key</label
            ><a-input-password
              v-model="draft.s3SecretKey"
              :placeholder="
                state.settings.hasSecretKey
                  ? '已保存，留空保留'
                  : 'S3 Secret Access Key'
              "
              :input-attrs="{
                id: 'cloud-sync-s3-secret',
                'aria-label': 'S3 Secret Key',
                autocomplete: 'new-password',
              }"
              @input="
                edited();
                secretEdited = true;
              "
            />
          </div>
        </template>
        <template v-else>
          <div class="cloud-sync-field">
            <label for="cloud-sync-github-owner">仓库所有者</label
            ><a-input
              v-model="draft.githubOwner"
              placeholder="GitHub 用户名或组织"
              :input-attrs="{
                id: 'cloud-sync-github-owner',
                'aria-label': 'GitHub 仓库所有者',
                autocomplete: 'off',
                spellcheck: false,
              }"
              @input="edited"
            />
          </div>
          <div class="cloud-sync-field">
            <label for="cloud-sync-github-repo">仓库名称</label
            ><a-input
              v-model="draft.githubRepo"
              placeholder="balancehub-sync"
              :input-attrs="{
                id: 'cloud-sync-github-repo',
                'aria-label': 'GitHub 仓库名称',
                autocomplete: 'off',
                spellcheck: false,
              }"
              @input="edited"
            />
          </div>
          <div class="cloud-sync-field">
            <label for="cloud-sync-github-branch">分支</label
            ><a-input
              v-model="draft.githubBranch"
              placeholder="main"
              :input-attrs="{
                id: 'cloud-sync-github-branch',
                'aria-label': 'GitHub 分支',
                autocomplete: 'off',
                spellcheck: false,
              }"
              @input="edited"
            />
          </div>
          <div class="cloud-sync-field">
            <label for="cloud-sync-github-token">个人访问令牌</label
            ><a-input-password
              v-model="draft.githubToken"
              :placeholder="
                state.settings.hasGithubToken
                  ? '已保存，留空保留'
                  : 'Fine-grained token，Contents 读写权限'
              "
              :input-attrs="{
                id: 'cloud-sync-github-token',
                'aria-label': 'GitHub 个人访问令牌',
                autocomplete: 'new-password',
              }"
              @input="
                edited();
                githubTokenEdited = true;
              "
            /><small
              >令牌需要该仓库 Contents 的读取与写入权限；仓库需已有提交和所选分支。</small
            >
          </div>
        </template>
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
          <label for="cloud-sync-directory">{{
            draft.backend === "webdav" ? "同步目录" : "同步前缀"
          }}</label
          ><a-input
            v-model="draft.remoteRoot"
            placeholder="BalanceHub"
            :input-attrs="{
              id: 'cloud-sync-directory',
              'aria-label': '同步目录或前缀',
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
            :disabled="busy || !draftReady"
            @click="submit('sync')"
            >保存并同步</a-button
          >
          <a-button
            :disabled="busy || !draftReady"
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
          和条件写入，测试连接会实际验证。WebDAV 使用应用密码；S3
          兼容存储使用 Access Key，并要求 s3:GetObject、s3:PutObject、s3:DeleteObject
          与 s3:ListBucket 权限；GitHub 使用具有该仓库 Contents
          读写权限的个人访问令牌，文件以提交方式写入所选分支。连接使用应用的网络代理，建议填写
          HTTPS 地址。
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
.cloud-sync-field > label,
.cloud-sync-field-label {
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
