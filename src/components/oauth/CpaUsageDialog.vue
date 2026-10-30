<script setup lang="ts">
import { computed, onScopeDispose, ref, watch } from "vue";
import { Copy, Eye, EyeOff, KeyRound } from "@lucide/vue";
import { Message } from "@arco-design/web-vue";
import type { Provider } from "../../stores/provider-types";
import type { CpaApiKeyUsage } from "../../stores/oauth-types";
import { readCpaApiKeyUsage } from "../../api/oauth-accounts";
import { quotaNumber } from "../../utils/oauth-display";
import { copyText } from "../../composables/useClipboard";
const props = defineProps<{ visible: boolean; provider: Provider }>();
const emit = defineEmits<{ close: [] }>();
const loading = ref(false);
const error = ref("");
const usage = ref<CpaApiKeyUsage[] | null>(null);
/// A key stays masked until its own row asks for it; the reveal is per row so
/// one credential never exposes the next.
const revealed = ref("");
let sequence = 0;
let disposed = false;
onScopeDispose(() => {
  disposed = true;
  sequence++;
});
watch(
  () => props.visible,
  (visible) => {
    if (visible) void load();
    else reset();
  },
  { immediate: true },
);
watch(
  () => props.provider.identity.id,
  () => {
    if (props.visible) void load();
  },
);
/// The gateway reports one bucket per credential; grouping keeps a provider
/// that holds several keys from repeating its name on every row.
const groups = computed(() => {
  const byProvider = new Map<string, CpaApiKeyUsage[]>();
  for (const entry of usage.value || []) {
    const rows = byProvider.get(entry.provider) || [];
    rows.push(entry);
    byProvider.set(entry.provider, rows);
  }
  return [...byProvider].map(([provider, rows]) => ({
    provider,
    rows,
    success: rows.reduce((sum, row) => sum + row.success, 0),
    failed: rows.reduce((sum, row) => sum + row.failed, 0),
  }));
});
const totals = computed(() =>
  (usage.value || []).reduce(
    (sum, entry) => ({
      keys: sum.keys + 1,
      success: sum.success + entry.success,
      failed: sum.failed + entry.failed,
    }),
    { keys: 0, success: 0, failed: 0 },
  ),
);
async function load() {
  usage.value = null;
  error.value = "";
  revealed.value = "";
  loading.value = true;
  const current = ++sequence;
  try {
    const result = await readCpaApiKeyUsage(props.provider.identity.id);
    if (!disposed && current === sequence) usage.value = result;
  } catch (cause) {
    if (!disposed && current === sequence) error.value = String(cause);
  } finally {
    if (current === sequence) loading.value = false;
  }
}
function maskId(entry: CpaApiKeyUsage) {
  return `${entry.provider}|${entry.key}`;
}
function maskedKey(key: string) {
  return key.length <= 12 ? "••••••" : `${key.slice(0, 6)}…${key.slice(-4)}`;
}
function toggleReveal(entry: CpaApiKeyUsage) {
  const id = maskId(entry);
  revealed.value = revealed.value === id ? "" : id;
}
async function copyKey(entry: CpaApiKeyUsage) {
  try {
    await copyText(entry.key);
    Message.success("已复制 API Key");
  } catch (cause) {
    if (!disposed) error.value = String(cause);
  }
}
/// The meter draws failures, so a fully green bar means every request worked.
function failedShare(success: number, failed: number) {
  const total = success + failed;
  return total ? Math.round((failed / total) * 100) : 0;
}
function reset() {
  sequence++;
  usage.value = null;
  error.value = "";
  revealed.value = "";
  // A read in flight belongs to the closed dialog; its own sequence can no
  // longer clear this, so the close does it instead of leaving it busy.
  loading.value = false;
}
</script>

<template>
  <a-modal
    :visible="visible"
    title="CPA 调用用量"
    width="720px"
    :footer="false"
    unmount-on-close
    @cancel="emit('close')"
  >
    <a-alert v-if="error" type="warning">{{ error }}</a-alert>
    <a-spin v-if="loading" tip="读取 CPA 调用用量" />
    <template v-else-if="usage">
      <p v-if="totals.keys" class="cpa-usage-summary">
        {{ totals.keys }} 个上游 API Key · 共
        {{ quotaNumber(totals.success + totals.failed) }} 次请求
        <template v-if="totals.failed"
          >· {{ quotaNumber(totals.failed) }} 次失败</template
        >
      </p>
      <div v-if="groups.length" class="cpa-usage-groups">
        <section
          v-for="group in groups"
          :key="group.provider"
          class="cpa-usage-group"
        >
          <header>
            <strong>{{ group.provider }}</strong
            ><span
              >{{ quotaNumber(group.success + group.failed) }} 次请求<template
                v-if="group.failed"
              >
                · {{ quotaNumber(group.failed) }} 次失败</template
              ></span
            >
          </header>
          <div class="cpa-usage-list">
            <div
              v-for="entry in group.rows"
              :key="`${entry.baseUrl}|${entry.key}`"
              class="cpa-usage-row"
            >
              <KeyRound :size="13" class="cpa-usage-icon" /><code
                :class="{
                  'is-revealed': revealed === maskId(entry),
                }"
                >{{ revealed === maskId(entry) ? entry.key : maskedKey(entry.key) }}</code
              >
              <span v-if="entry.baseUrl" class="cpa-usage-base" :title="entry.baseUrl">{{
                entry.baseUrl
              }}</span>
              <span class="cpa-usage-count"
                >成功 {{ quotaNumber(entry.success) }} · 失败
                {{ quotaNumber(entry.failed) }}</span
              >
              <div class="cpa-usage-meter" aria-hidden="true">
                <span
                  :style="{
                    transform: `scaleX(${failedShare(entry.success, entry.failed) / 100})`,
                  }"
                />
              </div>
              <div class="cpa-usage-actions">
                <button
                  type="button"
                  :title="revealed === maskId(entry) ? '隐藏密钥' : '显示密钥'"
                  :aria-label="revealed === maskId(entry) ? '隐藏密钥' : '显示密钥'"
                  @click="toggleReveal(entry)"
                >
                  <EyeOff v-if="revealed === maskId(entry)" :size="13" /><Eye
                    v-else
                    :size="13"
                  />
                </button>
                <button
                  type="button"
                  title="复制密钥"
                  aria-label="复制密钥"
                  @click="copyKey(entry)"
                >
                  <Copy :size="13" />
                </button>
              </div>
            </div>
          </div>
        </section>
      </div>
      <p v-else-if="!error" class="cpa-empty">
        网关没有上报内联 API Key 的调用记录。只有直接配置在网关内的 API Key
        凭证会计入这里；授权文件形式的账号在上面的授权列表里统计。
      </p>
    </template>
    <p v-else-if="!error" class="cpa-empty">未能读取 CPA 调用用量。</p>
  </a-modal>
</template>

<style scoped>
.cpa-empty {
  color: var(--color-text-3);
  font-size: 12px;
  margin: 0;
  line-height: 1.65;
}
.cpa-usage-summary {
  margin: 0 0 10px;
  color: var(--color-text-3);
  font-size: 11px;
  font-variant-numeric: tabular-nums;
}
.cpa-usage-groups {
  display: grid;
  gap: 12px;
  max-height: 58vh;
  overflow: auto;
}
.cpa-usage-group header {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 6px;
}
.cpa-usage-group header strong {
  color: var(--color-text-1);
  font-size: 12px;
  font-weight: 550;
}
.cpa-usage-group header span {
  color: var(--color-text-3);
  font-size: 11px;
  font-variant-numeric: tabular-nums;
}
.cpa-usage-list {
  display: grid;
  gap: 4px;
}
.cpa-usage-row {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
  padding: 5px 8px;
  border: 1px solid var(--color-border-2);
  border-radius: 8px;
  background: var(--color-fill-1);
}
.cpa-usage-icon {
  flex: 0 0 auto;
  color: var(--color-text-3);
}
.cpa-usage-row code {
  flex: 0 1 auto;
  min-width: 0;
  overflow: hidden;
  color: var(--color-text-2);
  font-family: var(--font-mono, ui-monospace, SFMono-Regular, monospace);
  font-size: 11px;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.cpa-usage-row code.is-revealed {
  white-space: normal;
  word-break: break-all;
}
.cpa-usage-base {
  flex: 1;
  min-width: 60px;
  overflow: hidden;
  color: var(--color-text-3);
  font-size: 11px;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.cpa-usage-count {
  flex: 0 0 auto;
  color: var(--color-text-3);
  font-size: 11px;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}
.cpa-usage-meter {
  flex: 0 0 46px;
  height: 4px;
  overflow: hidden;
  border-radius: 999px;
  background: color-mix(in srgb, rgb(var(--green-6)) 28%, transparent);
}
.cpa-usage-meter span {
  display: block;
  width: 100%;
  height: 100%;
  border-radius: inherit;
  background: rgb(var(--red-6));
  transform-origin: left center;
}
.cpa-usage-actions {
  display: flex;
  flex: 0 0 auto;
  align-items: center;
  gap: 2px;
}
.cpa-usage-actions button {
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
.cpa-usage-actions button:hover {
  background: var(--color-fill-2);
  color: var(--color-text-1);
}
</style>
