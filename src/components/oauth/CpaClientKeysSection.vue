<script setup lang="ts">
import { computed, onScopeDispose, ref, watch } from "vue";
import { ChevronDown, Copy, Eye, EyeOff, KeyRound, Trash2 } from "@lucide/vue";
import { Message } from "@arco-design/web-vue";
import type { Provider } from "../../stores/provider-types";
import type { CpaClientKeyAdded } from "../../stores/oauth-types";
import {
  addCpaClientKey,
  listCpaClientKeys,
  removeCpaClientKey,
} from "../../api/oauth-accounts";
import { confirmAction } from "../../composables/provider-credential-dialogs";
import { copyText } from "../../composables/useClipboard";

const props = defineProps<{ provider: Provider; revision: number }>();

/// Client keys are the gateway's downstream credentials, separate from the
/// management key this card connects with. They are read on demand.
const clientKeys = ref<string[] | null>(null);
const keysOpen = ref(false);
const keysLoading = ref(false);
const keysSaving = ref(false);
const keysError = ref("");
const keyDraft = ref("");
/// A key is masked by default; the row the user just added or revealed stays
/// readable until the section closes.
const revealedKey = ref("");
let keyRequest = 0;
/// 写操作只跟随站点/凭据版本失效，不被并发的列表读取打断。
let keysView = 0;
let disposed = false;
onScopeDispose(() => {
  disposed = true;
  keysView++;
});
/// 只有切换站点才需要清空列表。revision 变化只说明凭据版本变了：客户端
/// 密钥属于网关本身，后台换令牌不能把已读到的列表、刚粘贴的密钥或展开
/// 状态清掉。写入使用的是最新 revision，冲突会显示在面板里。
watch(
  () => props.provider.identity.id,
  () => {
    keyRequest++;
    keysView++;
    clientKeys.value = null;
    keysError.value = "";
    keysOpen.value = false;
    keyDraft.value = "";
    revealedKey.value = "";
    keysLoading.value = false;
    // 换站点后，上一次添加/删除不能再占用新列表的按钮状态。
    keysSaving.value = false;
  },
);
/// The collapsed line has to answer "do I have anything to hand a client?"
/// without opening the section.
const keysSummary = computed(() => {
  if (keysLoading.value) return "读取客户端密钥…";
  if (!clientKeys.value) return "查看或新增调用网关的密钥";
  return clientKeys.value.length
    ? `${clientKeys.value.length} 个密钥`
    : "还没有客户端密钥";
});
/// Keys are long and usually pasted once, so only the prefix and tail stay
/// readable while masked.
function maskedKey(key: string) {
  return key.length <= 12 ? "••••••" : `${key.slice(0, 6)}…${key.slice(-4)}`;
}
function toggleKeys() {
  keysOpen.value = !keysOpen.value;
  if (keysOpen.value && !clientKeys.value && !keysLoading.value) {
    void loadKeys();
  }
}
async function loadKeys() {
  const sequence = ++keyRequest;
  const id = props.provider.identity.id;
  keysLoading.value = true;
  keysError.value = "";
  try {
    const result = await listCpaClientKeys(id);
    if (!disposed && sequence === keyRequest) clientKeys.value = result.keys;
  } catch (cause) {
    if (!disposed && sequence === keyRequest) keysError.value = String(cause);
  } finally {
    if (sequence === keyRequest) keysLoading.value = false;
  }
}
async function addKey() {
  if (keysSaving.value || disposed) return;
  const id = props.provider.identity.id;
  const revision = props.revision;
  const view = keysView;
  keysSaving.value = true;
  keysError.value = "";
  try {
    const result: CpaClientKeyAdded = await addCpaClientKey(
      id,
      keyDraft.value.trim(),
      revision,
    );
    if (!sameKeyView(id, view)) return;
    // 作废可能还在途中的旧列表读取，避免它用没有新密钥的快照覆盖结果。
    keyRequest++;
    clientKeys.value = result.list.keys;
    keyDraft.value = "";
    /// A key this app minted exists nowhere else yet, so it stays visible
    /// until the section is closed instead of hiding behind the mask.
    revealedKey.value = result.value;
    Message.success("客户端密钥已添加");
  } catch (cause) {
    if (!disposed && view === keysView) keysError.value = String(cause);
  } finally {
    if (view === keysView) keysSaving.value = false;
  }
}
async function removeKey(key: string) {
  if (keysSaving.value || disposed) return;
  const id = props.provider.identity.id;
  const revision = props.revision;
  const view = keysView;
  keysSaving.value = true;
  keysError.value = "";
  try {
    const confirmed = await confirmAction(
      "删除客户端密钥",
      "使用该密钥的客户端会立即无法调用网关。网关上的上游授权不受影响。",
      "删除密钥",
      "danger",
    );
    if (!confirmed || !sameKeyView(id, view)) return;
    const result = await removeCpaClientKey(id, key, revision);
    if (!sameKeyView(id, view)) return;
    keyRequest++;
    clientKeys.value = result.keys;
    if (revealedKey.value === key) revealedKey.value = "";
    Message.success("客户端密钥已删除");
  } catch (cause) {
    if (!disposed && view === keysView) keysError.value = String(cause);
  } finally {
    if (view === keysView) keysSaving.value = false;
  }
}
/// 请求返回时只有仍是同一个站点、同一段列表视图才能写回；换站点或重新
/// 打开列表会让旧结果失效。凭据版本不参与判断：密钥属于网关本身，换令牌
/// 不会让已完成的添加结果变得无效。
function sameKeyView(id: string, view: number) {
  return !disposed && view === keysView && props.provider.identity.id === id;
}
async function copyKey(key: string) {
  try {
    await copyText(key);
    Message.success("密钥已复制");
  } catch (cause) {
    if (!disposed) keysError.value = String(cause);
  }
}
function toggleReveal(key: string) {
  revealedKey.value = revealedKey.value === key ? "" : key;
}
</script>
<template>
  <section class="cpa-keys" aria-label="客户端密钥">
    <button
      type="button"
      class="cpa-keys-head"
      :aria-expanded="keysOpen"
      @click="toggleKeys"
    >
      <span
        ><strong>客户端密钥</strong><small>{{ keysSummary }}</small></span
      ><ChevronDown :size="13" class="cpa-keys-caret" :class="{ open: keysOpen }" />
    </button>
    <div v-if="keysOpen" class="cpa-keys-body">
      <a-alert v-if="keysError" type="warning">{{ keysError }}</a-alert>
      <a-spin v-if="keysLoading" tip="读取客户端密钥" />
      <template v-else>
        <div v-if="clientKeys?.length" class="cpa-key-list">
          <div v-for="key in clientKeys" :key="key" class="cpa-key-row">
            <KeyRound :size="13" class="cpa-key-icon" /><code
              :class="{ 'is-revealed': revealedKey === key }"
              >{{ revealedKey === key ? key : maskedKey(key) }}</code
            >
            <div class="cpa-key-actions">
              <button
                type="button"
                :title="revealedKey === key ? '隐藏密钥' : '显示密钥'"
                :aria-label="revealedKey === key ? '隐藏密钥' : '显示密钥'"
                @click="toggleReveal(key)"
              >
                <EyeOff v-if="revealedKey === key" :size="13" /><Eye
                  v-else
                  :size="13"
                />
              </button>
              <button
                type="button"
                title="复制密钥"
                aria-label="复制密钥"
                @click="copyKey(key)"
              >
                <Copy :size="13" />
              </button>
              <button
                type="button"
                class="is-danger"
                title="删除密钥"
                aria-label="删除密钥"
                :disabled="keysSaving"
                @click="removeKey(key)"
              >
                <Trash2 :size="13" />
              </button>
            </div>
          </div>
        </div>
        <p v-else class="cpa-key-empty">
          网关上还没有客户端密钥，客户端暂时无法通过它调用模型。
        </p>
        <div class="cpa-key-add">
          <a-input
            v-model="keyDraft"
            placeholder="粘贴自有密钥，留空则由本机生成"
            allow-clear
            :disabled="keysSaving"
            @press-enter="addKey"
          /><a-button
            type="primary"
            size="mini"
            :loading="keysSaving"
            @click="addKey"
            >{{ keyDraft.trim() ? "添加" : "生成密钥" }}</a-button
          >
        </div>
        <p class="cpa-key-hint">
          客户端把密钥填进 API Key 即可通过该网关调用已配置的授权；删除后使用它的客户端会立即失效。
        </p>
      </template>
    </div>
  </section>
</template>
<style scoped>
.cpa-keys {
  border: 1px solid var(--color-border-2);
  border-radius: 10px;
  background: var(--color-fill-1);
}
.cpa-keys-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  width: 100%;
  padding: 10px 13px;
  border: 0;
  background: none;
  font: inherit;
  text-align: left;
  cursor: pointer;
}
.cpa-keys-head > span {
  display: flex;
  align-items: baseline;
  gap: 9px;
  min-width: 0;
}
.cpa-keys-head strong {
  color: var(--color-text-1);
  font-size: 12px;
  font-weight: 550;
  white-space: nowrap;
}
.cpa-keys-head small {
  overflow: hidden;
  color: var(--color-text-3);
  font-size: 11px;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.cpa-keys-caret {
  flex: 0 0 auto;
  color: var(--color-text-3);
  transition: transform 0.15s ease;
}
.cpa-keys-caret.open {
  transform: rotate(180deg);
}
.cpa-keys-body {
  display: grid;
  gap: 9px;
  padding: 0 13px 13px;
}
.cpa-key-list {
  display: grid;
  gap: 4px;
}
.cpa-key-row {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
  padding: 5px 8px;
  border: 1px solid var(--color-border-2);
  border-radius: 8px;
  background: var(--color-bg-2);
}
.cpa-key-icon {
  flex: 0 0 auto;
  color: var(--color-text-3);
}
.cpa-key-row code {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  color: var(--color-text-2);
  font-family: var(--font-mono, ui-monospace, SFMono-Regular, monospace);
  font-size: 11px;
  text-overflow: ellipsis;
  white-space: nowrap;
}
/* A revealed key has to be readable in full, so it wraps instead of clipping. */
.cpa-key-row code.is-revealed {
  white-space: normal;
  word-break: break-all;
}
.cpa-key-actions {
  display: flex;
  flex: 0 0 auto;
  align-items: center;
  gap: 2px;
}
.cpa-key-actions button {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  border: 0;
  border-radius: 6px;
  background: none;
  color: var(--color-text-3);
  cursor: pointer;
}
.cpa-key-actions button:hover:not(:disabled) {
  background: var(--color-fill-2);
  color: var(--color-text-1);
}
.cpa-key-actions button.is-danger:hover:not(:disabled) {
  background: rgba(211, 55, 65, 0.1);
  color: #d33741;
}
.cpa-key-actions button:disabled {
  cursor: default;
  opacity: 0.5;
}
.cpa-key-add {
  display: flex;
  align-items: center;
  gap: 8px;
}
.cpa-key-add .arco-input-wrapper {
  flex: 1;
  min-width: 0;
}
.cpa-key-hint {
  margin: 0;
  color: var(--color-text-3);
  font-size: 11px;
  line-height: 1.6;
}
.cpa-key-empty {
  margin: 0;
  color: var(--color-text-3);
  font-size: 12px;
  line-height: 1.65;
}
</style>
