<script setup lang="ts">
import { onScopeDispose, ref } from "vue";
import { ExternalLink, Copy, CheckCircle2, LoaderCircle } from "@lucide/vue";
import { Message } from "@arco-design/web-vue";
import type { OAuthLoginTask } from "../../stores/oauth-types";
import {
  cancelOAuthLogin,
  listOAuthLogins,
  openOAuthPage,
  retryOAuthSave,
  submitOAuthCallback,
} from "../../api/oauth-accounts";
import { useOAuthAccountStore } from "../../stores/oauth-accounts";
import { copyText } from "../../composables/useClipboard";
import { accountTimestamp, oauthVendorLabel } from "../../utils/oauth-display";
const props = defineProps<{ task: OAuthLoginTask }>();
const store = useOAuthAccountStore();
const callback = ref("");
const busy = ref(false);
const error = ref("");
let disposed = false;
onScopeDispose(() => {
  disposed = true;
});
async function run(action: () => Promise<void>) {
  if (busy.value) return;
  busy.value = true;
  error.value = "";
  try {
    await action();
    if (!disposed) {
      callback.value = "";
      for (const task of await listOAuthLogins()) store.accept(task);
    }
  } catch (cause) {
    if (!disposed) error.value = String(cause);
  } finally {
    busy.value = false;
  }
}
async function copyCode() {
  try {
    await copyText(props.task.userCode || "");
    Message.success("设备码已复制");
  } catch (cause) {
    Message.error(String(cause));
  }
}
</script>
<template>
  <div class="oauth-task-panel">
    <div class="oauth-task-heading">
      <CheckCircle2 v-if="task.phase === 'completed'" :size="25" /><LoaderCircle
        v-else-if="['starting', 'exchanging'].includes(task.phase)"
        class="oauth-task-spin"
        :size="25"
      />
      <div>
        <strong>{{ oauthVendorLabel(task.vendor) }} 授权</strong>
        <p role="status">{{ task.message }}</p>
      </div>
    </div>
    <div v-if="task.userCode" class="oauth-device-code">
      <span>核对浏览器中的设备码</span
      ><button type="button" @click="copyCode">
        <code>{{ task.userCode }}</code
        ><Copy :size="16" />
      </button>
    </div>
    <a-button
      v-if="task.authorizationUrl"
      :loading="busy"
      @click="run(() => openOAuthPage(task.id))"
      ><template #icon><ExternalLink :size="15" /></template
      >打开授权页面</a-button
    >
    <div v-if="task.acceptsCallback" class="oauth-callback">
      <label for="oauth-callback">{{
        task.vendor === "gemini"
          ? "浏览器返回的授权码"
          : "未自动返回时，粘贴完整回调地址"
      }}</label
      ><a-textarea
        id="oauth-callback"
        v-model="callback"
        :auto-size="{ minRows: 2, maxRows: 4 }"
        :placeholder="
          task.vendor === 'gemini'
            ? '粘贴 Google 授权页面显示的代码'
            : 'http://localhost:…?code=…&state=…'
        "
        :max-length="8192"
      /><a-button
        type="primary"
        :disabled="!callback.trim()"
        :loading="busy"
        @click="run(() => submitOAuthCallback(task.id, callback))"
        >完成授权</a-button
      >
    </div>
    <a-alert v-if="error" type="error">{{ error }}</a-alert>
    <div class="oauth-task-footer">
      <span v-if="task.canCancel"
        >有效期至 {{ accountTimestamp(task.expiresAt) }}</span
      ><span v-else>关闭面板后仍可在任务中心查看</span
      ><a-button
        v-if="task.canCancel"
        :loading="busy"
        @click="run(() => cancelOAuthLogin(task.id))"
        >取消授权</a-button
      ><a-button
        v-if="task.canRetrySave"
        type="primary"
        :loading="busy"
        @click="run(() => retryOAuthSave(task.id))"
        >重试保存</a-button
      ><a-button
        v-if="task.phase === 'completed' && task.providerId"
        type="primary"
        @click="store.openAccount(task.providerId)"
        >查看账号</a-button
      >
    </div>
  </div>
</template>
<style scoped>
.oauth-task-panel {
  display: grid;
  gap: 20px;
}
.oauth-task-heading {
  display: flex;
  gap: 14px;
  color: rgb(var(--primary-6));
}
.oauth-task-heading strong {
  color: var(--color-text-1);
  font-size: 16px;
  font-weight: 550;
}
.oauth-task-heading p {
  margin: 8px 0 0;
  color: var(--color-text-2);
  line-height: 1.7;
  overflow-wrap: anywhere;
}
.oauth-device-code {
  padding: 20px;
  display: grid;
  gap: 10px;
  justify-items: center;
  background: var(--color-fill-2);
  border-radius: 10px;
}
.oauth-device-code > span {
  color: var(--color-text-3);
  font-size: 12px;
}
.oauth-device-code button {
  display: flex;
  align-items: center;
  gap: 14px;
  border: 0;
  background: none;
  color: var(--color-text-1);
  cursor: pointer;
}
.oauth-device-code code {
  font-size: 25px;
  letter-spacing: 0.1em;
}
.oauth-callback {
  display: grid;
  gap: 10px;
}
.oauth-callback label {
  font-size: 12px;
  color: var(--color-text-2);
}
.oauth-callback :deep(.arco-btn) {
  justify-self: end;
}
.oauth-task-footer {
  display: flex;
  align-items: center;
  gap: 12px;
  border-top: 1px solid var(--color-border-2);
  padding-top: 16px;
}
.oauth-task-footer > span {
  flex: 1;
  color: var(--color-text-3);
  font-size: 12px;
}
.oauth-task-spin {
  animation: oauth-spin 1s linear infinite;
}
@keyframes oauth-spin {
  to {
    transform: rotate(360deg);
  }
}
@media (prefers-reduced-motion: reduce) {
  .oauth-task-spin {
    animation: none;
  }
}
</style>
