<script setup lang="ts">
import { computed, onScopeDispose, ref, watch } from "vue";
import { Copy, ExternalLink, LoaderCircle } from "@lucide/vue";
import { Message } from "@arco-design/web-vue";
import type { Provider } from "../../stores/provider-types";
import type { CpaLoginProvider, CpaLoginStart } from "../../stores/oauth-types";
import {
  cancelCpaLogin,
  getCpaLoginStatus,
  listCpaLoginProviders,
  openCpaLoginUrl,
  startCpaLogin,
  submitCpaLoginCallback,
} from "../../api/oauth-accounts";
import { copyText } from "../../composables/useClipboard";
import { accountTimestamp } from "../../utils/oauth-display";

const props = defineProps<{ visible: boolean; provider: Provider }>();
const emit = defineEmits<{ close: []; completed: [] }>();

/// Polling cadence for the pending session. The gateway asks the provider on
/// its own, so this only reads the answer it already has.
const POLL_MS = 2500;

const providers = ref<CpaLoginProvider[]>([]);
const loading = ref(false);
const error = ref("");
const selected = ref("");
const starting = ref(false);
const startError = ref("");
/// The session the gateway is holding, the service it belongs to, and where it
/// stands.
const login = ref<CpaLoginStart | null>(null);
const started = ref<CpaLoginProvider | null>(null);
const phase = ref<"idle" | "waiting" | "completed" | "failed">("idle");
const statusError = ref("");
const callback = ref("");
const submitting = ref(false);
const callbackError = ref("");
let providerRequest = 0;
/// Bumped whenever the session goes away, so a status or callback answer that
/// arrives afterwards cannot land in a session that no longer exists.
let sessionRequest = 0;
let polling = false;
let pollTimer: ReturnType<typeof setInterval> | undefined;
let disposed = false;
onScopeDispose(() => {
  disposed = true;
  stopPolling();
  /// Leaving the dialog must not leave the gateway waiting: a pending browser
  /// flow holds a callback port until it is cancelled or expires.
  void abandon();
});
watch(
  () => [props.visible, props.provider.identity.id],
  ([visible]) => {
    /// Closing the dialog or switching gateways abandons whatever the old
    /// gateway was still waiting for; the session is cancelled, not orphaned.
    void abandon();
    providerRequest++;
    resetSession();
    providers.value = [];
    selected.value = "";
    error.value = "";
    startError.value = "";
    loading.value = false;
    if (visible) void load();
  },
  { immediate: true },
);

const current = computed(() =>
  providers.value.find((entry) => entry.id === selected.value),
);
const isDevice = computed(() => login.value?.flow === "device");
const expiresAt = computed(() => {
  const start = login.value;
  return start?.expiresIn ? Math.floor(Date.now() / 1000) + start.expiresIn : null;
});
const pending = computed(() => phase.value === "waiting");

function resetSession() {
  stopPolling();
  sessionRequest++;
  login.value = null;
  started.value = null;
  phase.value = "idle";
  statusError.value = "";
  callback.value = "";
  callbackError.value = "";
}
async function load() {
  const sequence = ++providerRequest;
  const id = props.provider.identity.id;
  loading.value = true;
  try {
    const result = await listCpaLoginProviders(id);
    if (disposed || sequence !== providerRequest) return;
    providers.value = result;
    selected.value = result[0]?.id ?? "";
  } catch (cause) {
    if (!disposed && sequence === providerRequest) error.value = String(cause);
  } finally {
    if (sequence === providerRequest) loading.value = false;
  }
}
async function start() {
  if (!current.value || starting.value || login.value) return;
  const id = props.provider.identity.id;
  const entry = current.value;
  starting.value = true;
  startError.value = "";
  try {
    const result = await startCpaLogin(id, entry.id);
    if (disposed) {
      /// The dialog closed while the gateway was still generating the URL;
      /// the session it just created is cancelled instead of left waiting.
      void cancelCpaLogin(id, result.state).catch(() => {});
      return;
    }
    started.value = entry;
    login.value = result;
    phase.value = "waiting";
    statusError.value = "";
    startPolling();
    void poll();
  } catch (cause) {
    if (!disposed) startError.value = String(cause);
  } finally {
    starting.value = false;
  }
}
function stopPolling() {
  if (pollTimer) clearInterval(pollTimer);
  pollTimer = undefined;
}
function startPolling() {
  stopPolling();
  pollTimer = setInterval(() => void poll(), POLL_MS);
}
async function poll() {
  const session = login.value;
  if (!session || phase.value !== "waiting" || polling) return;
  polling = true;
  const sequence = sessionRequest;
  const id = props.provider.identity.id;
  try {
    const result = await getCpaLoginStatus(
      id,
      session.state,
      session.apiVersion,
    );
    if (disposed || sequence !== sessionRequest) return;
    if (result.status === "waiting") {
      statusError.value = "";
      return;
    }
    stopPolling();
    if (result.status === "completed") {
      phase.value = "completed";
      emit("completed");
      return;
    }
    phase.value = "failed";
    statusError.value = result.message;
  } catch (cause) {
    /// A single failed read is not a failed session: the gateway may be busy
    /// or the network may blink, so polling continues and the error is shown.
    if (!disposed && sequence === sessionRequest) statusError.value = String(cause);
  } finally {
    polling = false;
  }
}
async function copyUrl() {
  const url = login.value?.url;
  if (!url) return;
  try {
    await copyText(url);
    Message.success("授权链接已复制");
  } catch (cause) {
    Message.error(String(cause));
  }
}
async function open() {
  const session = login.value;
  if (!session) return;
  try {
    await openCpaLoginUrl(props.provider.identity.id, session.url);
  } catch (cause) {
    Message.error(String(cause));
  }
}
async function copyCode() {
  const code = login.value?.userCode;
  if (!code) return;
  try {
    await copyText(code);
    Message.success("设备码已复制");
  } catch (cause) {
    Message.error(String(cause));
  }
}
async function submit() {
  const session = login.value;
  if (!session || submitting.value || !callback.value.trim()) return;
  submitting.value = true;
  callbackError.value = "";
  const sequence = sessionRequest;
  try {
    await submitCpaLoginCallback(
      props.provider.identity.id,
      started.value?.id ?? "",
      session.state,
      session.apiVersion,
      callback.value,
    );
    if (disposed || sequence !== sessionRequest) return;
    callback.value = "";
    Message.success("回调已提交，等待网关换取令牌");
    if (phase.value === "waiting") void poll();
  } catch (cause) {
    if (!disposed && sequence === sessionRequest) callbackError.value = String(cause);
  } finally {
    submitting.value = false;
  }
}
async function cancel() {
  const session = login.value;
  if (!session) return;
  resetSession();
  try {
    await cancelCpaLogin(props.provider.identity.id, session.state);
  } catch (cause) {
    if (!disposed) Message.error(String(cause));
  }
}
/// Best-effort cleanup used when the dialog is closed or unmounted, including
/// while the gateway is still generating the first URL.
async function abandon() {
  const session = login.value;
  if (!session || phase.value !== "waiting") return;
  resetSession();
  await cancelCpaLogin(props.provider.identity.id, session.state).catch(
    () => {},
  );
}
</script>
<template>
  <a-modal
    :visible="visible"
    title="在网关上添加授权"
    width="560px"
    :footer="false"
    unmount-on-close
    @cancel="emit('close')"
  >
    <div class="cpa-login-dialog">
      <a-alert v-if="error" type="warning">{{ error }}</a-alert>
      <template v-if="!login">
        <div class="cpa-login-field">
          <span>授权服务</span>
          <a-select
            v-model="selected"
            :loading="loading"
            :disabled="loading || !providers.length"
            placeholder="选择授权服务"
          >
            <a-option
              v-for="entry in providers"
              :key="entry.id"
              :value="entry.id"
              >{{ entry.label
              }}<small v-if="entry.plugin"> · 插件 {{ entry.plugin }}</small></a-option
            >
          </a-select>
        </div>
        <p v-if="loading" class="cpa-login-hint">读取网关支持的授权服务…</p>
        <p v-else-if="!providers.length" class="cpa-login-hint">
          网关未报告可用的授权服务，请先更新 CPA。
        </p>
        <p v-else class="cpa-login-hint">
          授权在网关上进行：网关生成链接并保存令牌，本机不保存这份账号。网关与本机不是同一台机器时，浏览器最后会停在一个打不开的回调地址，把地址栏里的完整链接粘贴回来即可。
        </p>
        <a-alert v-if="startError" type="warning">{{ startError }}</a-alert>
        <div class="cpa-login-actions">
          <a-button
            type="primary"
            :loading="starting"
            :disabled="!selected || loading"
            @click="start"
            >开始授权</a-button
          >
        </div>
      </template>
      <template v-else>
        <div class="cpa-login-status" role="status">
          <LoaderCircle
            v-if="pending"
            :size="19"
            class="cpa-login-spin"
          /><template v-if="pending">
            等待在浏览器中完成{{ started?.label || "" }}授权
          </template>
          <template v-else-if="phase === 'completed'">授权已完成</template>
          <template v-else>授权未完成</template>
        </div>
        <a-alert v-if="statusError" type="warning">{{ statusError }}</a-alert>
        <div v-if="login.userCode" class="cpa-login-device">
          <span>在浏览器中核对并输入设备码</span
          ><button type="button" @click="copyCode">
            <code>{{ login.userCode }}</code
            ><Copy :size="15" />
          </button>
        </div>
        <div class="cpa-login-url">
          <p>{{ login.url }}</p>
          <div class="cpa-login-actions">
            <a-button
              type="primary"
              :disabled="phase !== 'waiting'"
              @click="open"
              ><template #icon><ExternalLink :size="14" /></template
              >打开授权页面</a-button
            ><a-button :disabled="phase !== 'waiting'" @click="copyUrl"
              ><template #icon><Copy :size="14" /></template>复制链接</a-button
            >
          </div>
        </div>
        <template v-if="!isDevice && phase === 'waiting'">
          <div class="cpa-login-field">
            <span>回调地址或授权码</span>
            <a-textarea
              v-model="callback"
              :auto-size="{ minRows: 2, maxRows: 4 }"
              :max-length="8192"
              placeholder="浏览器停在无法访问的回调地址时，粘贴地址栏里的完整链接"
            />
          </div>
          <a-alert v-if="callbackError" type="warning">{{
            callbackError
          }}</a-alert>
          <div class="cpa-login-actions">
            <a-button
              type="primary"
              :disabled="!callback.trim()"
              :loading="submitting"
              @click="submit"
              >提交回调</a-button
            >
          </div>
        </template>
        <div class="cpa-login-footer">
          <span v-if="expiresAt && pending"
            >有效期至 {{ accountTimestamp(expiresAt) }}</span
          ><span v-else />
          <!-- A session that already ended is dropped the same way: the
               gateway forgets it and the picker comes back. -->
          <a-button v-if="phase !== 'completed'" @click="cancel">{{
            pending ? "取消授权" : "重新开始"
          }}</a-button>
        </div>
      </template>
    </div>
  </a-modal>
</template>
<style scoped>
.cpa-login-dialog {
  display: grid;
  gap: 12px;
}
.cpa-login-field {
  display: grid;
  gap: 6px;
}
.cpa-login-field > span {
  color: var(--color-text-2);
  font-size: 12px;
}
.cpa-login-hint {
  margin: 0;
  color: var(--color-text-3);
  font-size: 11px;
  line-height: 1.7;
}
.cpa-login-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
.cpa-login-status {
  display: flex;
  align-items: center;
  gap: 8px;
  color: var(--color-text-1);
  font-size: 12px;
}
.cpa-login-spin {
  color: rgb(var(--primary-6));
  animation: cpa-login-spin 1s linear infinite;
}
.cpa-login-device {
  display: grid;
  gap: 6px;
  justify-items: center;
  padding: 12px;
  border-radius: 9px;
  background: var(--color-fill-2);
}
.cpa-login-device > span {
  color: var(--color-text-3);
  font-size: 11px;
}
.cpa-login-device button {
  display: flex;
  align-items: center;
  gap: 10px;
  border: 0;
  background: none;
  color: var(--color-text-1);
  cursor: pointer;
}
.cpa-login-device code {
  font-size: 20px;
  letter-spacing: 0.12em;
}
.cpa-login-url {
  display: grid;
  gap: 8px;
}
.cpa-login-url p {
  margin: 0;
  max-height: 66px;
  overflow: auto;
  padding: 8px 10px;
  border: 1px solid var(--color-border-2);
  border-radius: 8px;
  background: var(--color-fill-1);
  color: var(--color-text-2);
  font-size: 11px;
  line-height: 1.6;
  overflow-wrap: anywhere;
}
.cpa-login-footer {
  display: flex;
  align-items: center;
  gap: 10px;
  padding-top: 10px;
  border-top: 1px solid var(--color-border-2);
}
.cpa-login-footer > span {
  flex: 1;
  color: var(--color-text-3);
  font-size: 11px;
}
@keyframes cpa-login-spin {
  to {
    transform: rotate(360deg);
  }
}
@media (prefers-reduced-motion: reduce) {
  .cpa-login-spin {
    animation: none;
  }
}
</style>
