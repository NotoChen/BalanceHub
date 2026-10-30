<script setup lang="ts">
import { computed, onScopeDispose, ref } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { FileUp, ArrowRight } from "@lucide/vue";
import { Message } from "@arco-design/web-vue";
import {
  applyOAuthImport,
  discardOAuthImport,
  previewOAuthImport,
} from "../../api/oauth-accounts";
import { useProviderStore } from "../../stores/providers";
import type { OAuthImportPreview, OAuthVendor } from "../../stores/oauth-types";
import {
  oauthVendors,
  oauthVendorLabel,
  accountTimestamp,
} from "../../utils/oauth-display";
import AgentCliIcon from "../AgentCliIcon.vue";

const props = defineProps<{ initialPreview?: OAuthImportPreview | null }>();
const emit = defineEmits<{ imported: [id: string | null] }>();
const store = useProviderStore();
const preview = ref<OAuthImportPreview | null>(props.initialPreview || null);
const selected = ref<string[]>(
  preview.value?.entries.filter((e) => !e.duplicateId).map((e) => e.id) || [],
);
const vendor = ref<OAuthVendor | undefined>();
const text = ref("");
const busy = ref(false);
const error = ref("");
const replaceExisting = ref(false);
const manageRefresh = ref(false);
let revision = 0;
let disposed = false;
const hasDuplicate = computed(() =>
  preview.value?.entries.some((e) => e.duplicateId),
);

function toggleEntry(id: string, checked: boolean | string | number) {
  selected.value = checked
    ? [...new Set([...selected.value, id])]
    : selected.value.filter((value) => value !== id);
}
function releasePreview() {
  if (preview.value)
    void discardOAuthImport(preview.value.token).catch(() => {});
  preview.value = null;
}
onScopeDispose(() => {
  disposed = true;
  revision++;
  releasePreview();
});

async function readFiles(fromText = false) {
  if (busy.value) return;
  const request = ++revision;
  busy.value = true;
  error.value = "";
  try {
    const chosen = fromText
      ? []
      : await open({
          title: "导入账号授权文件",
          multiple: true,
          directory: false,
          filters: [{ name: "JSON 授权", extensions: ["json"] }],
        });
    if (disposed || request !== revision || (!fromText && !chosen)) return;
    const paths = Array.isArray(chosen) ? chosen : chosen ? [chosen] : [];
    const result = await previewOAuthImport(
      fromText ? [{ name: "pasted.json", content: text.value }] : [],
      paths,
      vendor.value || null,
    );
    if (disposed || request !== revision) {
      void discardOAuthImport(result.token).catch(() => {});
      return;
    }
    releasePreview();
    preview.value = result;
    selected.value = result.entries
      .filter((entry) => !entry.duplicateId)
      .map((entry) => entry.id);
    text.value = "";
  } catch (cause) {
    if (!disposed && request === revision) error.value = String(cause);
  } finally {
    if (request === revision) busy.value = false;
  }
}
async function importSelected() {
  if (busy.value || !preview.value || !selected.value.length) return;
  const request = ++revision;
  busy.value = true;
  error.value = "";
  const token = preview.value.token;
  try {
    const providers = await applyOAuthImport({
      token,
      entryIds: selected.value,
      replaceExisting: replaceExisting.value,
      manageRefresh: manageRefresh.value,
    });
    store.upsertProviders(providers);
    if (providers.length)
      void store
        .refreshByIds(providers.map((provider) => provider.identity.id))
        .catch(() => {});
    if (!disposed && request === revision) {
      preview.value = null;
      Message.success(
        providers.length
          ? `已导入 ${providers.length} 个账号`
          : "已有账号已保留，没有新增账号",
      );
      emit("imported", providers[0]?.identity.id || null);
    }
  } catch (cause) {
    if (!disposed && request === revision) {
      error.value = `${String(cause)}；请重新读取文件后再试`;
      releasePreview();
    }
  } finally {
    if (request === revision) busy.value = false;
  }
}
</script>

<template>
  <div class="oauth-import-panel">
    <div class="oauth-import-entry">
      <div>
        <strong>导入授权文件</strong>
        <p>
          支持 BalanceHub、CPA、CC Switch、NewAPI、Sub2API、Magpie 与原生 CLI
          格式。
        </p>
      </div>
      <a-button type="primary" :loading="busy" @click="readFiles()"
        ><template #icon><FileUp :size="16" /></template>选择文件</a-button
      >
    </div>
    <details class="oauth-paste">
      <summary>粘贴授权内容 / 指定服务</summary>
      <div class="oauth-paste-content">
        <a-select
          v-model="vendor"
          allow-clear
          placeholder="自动识别服务"
          :options="oauthVendors"
        /><a-textarea
          v-model="text"
          :auto-size="{ minRows: 4, maxRows: 7 }"
          placeholder="粘贴原始授权 JSON"
          :max-length="4194304"
        /><a-button :disabled="!text.trim() || busy" @click="readFiles(true)"
          >预览内容</a-button
        >
      </div>
    </details>
    <a-alert v-if="error" type="error">{{ error }}</a-alert>
    <template v-if="preview">
      <div v-if="preview.errors.length" class="oauth-import-errors">
        <p v-for="(message, index) in preview.errors" :key="index">
          {{ message }}
        </p>
      </div>
      <div class="oauth-import-list" aria-label="待导入账号">
        <label
          v-for="entry in preview.entries"
          :key="entry.id"
          class="oauth-import-row"
          :class="{ selected: selected.includes(entry.id) }"
        >
          <a-checkbox
            :model-value="selected.includes(entry.id)"
            @change="toggleEntry(entry.id, $event === true)"
            :disabled="!!entry.duplicateId && !replaceExisting"
          />
          <AgentCliIcon :kind="entry.vendor" :size="22" />
          <span class="oauth-import-account"
            ><strong>{{ entry.label }}</strong
            ><small
              >{{ oauthVendorLabel(entry.vendor)
              }}<template v-if="entry.accountId">
                · {{ entry.accountId }}</template
              ><template v-if="entry.plan"> · {{ entry.plan }}</template></small
            ><small
              >{{ entry.source }} ·
              {{
                entry.expiresAt
                  ? `到期 ${accountTimestamp(entry.expiresAt)}`
                  : "未提供到期时间"
              }}</small
            ></span
          >
          <span class="oauth-import-tag">{{
            entry.duplicateId ? "已存在" : "新增"
          }}</span>
        </label>
        <p v-if="!preview.entries.length" class="oauth-muted">
          未发现可导入的 OAuth 账号。请选择包含原始令牌的授权文件。
        </p>
      </div>
      <div class="oauth-import-options">
        <a-checkbox
          v-if="hasDuplicate"
          v-model="replaceExisting"
          @change="
            selected = preview!.entries
              .filter((e) => replaceExisting || !e.duplicateId)
              .map((e) => e.id)
          "
          >更新已有账号的授权</a-checkbox
        ><a-checkbox v-model="manageRefresh">由本机续期授权</a-checkbox>
        <p>
          其他应用仍在使用这些授权时，请先保持此项关闭；需要由 BalanceHub
          接管时，再暂停原应用续期。
        </p>
      </div>
      <div class="oauth-panel-footer">
        <span>{{ selected.length }} 个账号已选</span
        ><a-button
          type="primary"
          :disabled="!selected.length"
          :loading="busy"
          @click="importSelected"
          >导入所选账号<template #icon><ArrowRight :size="15" /></template
        ></a-button>
      </div>
    </template>
  </div>
</template>

<style scoped>
.oauth-import-panel {
  display: grid;
  gap: 18px;
}
.oauth-import-entry {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 24px;
  padding: 18px;
  background: var(--color-fill-1);
  border: 1px solid var(--color-border-2);
  border-radius: 10px;
}
.oauth-import-entry strong {
  color: var(--color-text-1);
  font-size: 15px;
  font-weight: 550;
}
.oauth-import-entry p {
  margin: 7px 0 0;
  line-height: 1.6;
  font-size: 12px;
  color: var(--color-text-3);
}
.oauth-import-entry :deep(.arco-btn) {
  flex-shrink: 0;
}
.oauth-paste summary {
  cursor: pointer;
  font-size: 12px;
  color: var(--color-text-3);
}
.oauth-paste-content {
  display: grid;
  gap: 10px;
  margin-top: 12px;
}
.oauth-paste-content :deep(.arco-select) {
  width: 200px;
}
.oauth-paste-content :deep(.arco-btn) {
  justify-self: end;
}
.oauth-import-list {
  display: grid;
  gap: 7px;
  max-height: 320px;
  overflow: auto;
}
.oauth-import-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px;
  border: 1px solid var(--color-border-2);
  border-radius: 8px;
  cursor: pointer;
}
.oauth-import-row.selected {
  border-color: rgb(var(--primary-4));
  background: var(--color-primary-light-1);
}
.oauth-import-account {
  display: grid;
  gap: 5px;
  flex: 1;
  min-width: 0;
}
.oauth-import-account strong {
  font-size: 13px;
  font-weight: 500;
  color: var(--color-text-1);
  overflow-wrap: anywhere;
}
.oauth-import-account small {
  color: var(--color-text-3);
  font-size: 11px;
  overflow-wrap: anywhere;
}
.oauth-import-tag {
  font-size: 11px;
  flex-shrink: 0;
  color: var(--color-text-3);
}
.oauth-import-errors {
  padding: 10px 14px;
  background: var(--color-warning-light-1);
  border-radius: 8px;
  color: var(--color-text-2);
  font-size: 12px;
}
.oauth-import-errors p {
  margin: 4px 0;
}
.oauth-import-options {
  display: grid;
  gap: 9px;
}
.oauth-import-options p {
  margin: 0;
  color: var(--color-text-3);
  font-size: 12px;
  line-height: 1.6;
}
.oauth-panel-footer {
  border-top: 1px solid var(--color-border-2);
  padding-top: 14px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.oauth-panel-footer > span,
.oauth-muted {
  color: var(--color-text-3);
  font-size: 12px;
}
</style>
