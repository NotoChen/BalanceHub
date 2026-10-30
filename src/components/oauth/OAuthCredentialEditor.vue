<script setup lang="ts">
import { computed, onScopeDispose, ref, watch } from "vue";
import { Message } from "@arco-design/web-vue";
import ContentEditor from "../ContentEditor.vue";
import FileEditorModal from "../FileEditorModal.vue";
import {
  getOAuthCredential,
  replaceOAuthCredential,
} from "../../api/oauth-accounts";
import type { Provider } from "../../stores/provider-types";
import { confirmAction } from "../../composables/provider-credential-dialogs";
const props = defineProps<{ provider: Provider; visible: boolean }>();
const emit = defineEmits<{ close: []; saved: [provider: Provider] }>();
const original = ref("");
const draft = ref("");
const loading = ref(false);
const saving = ref(false);
const error = ref("");
const revision = computed(() => props.provider.auth.credentialRevision || 0);
const changed = computed(() => draft.value !== original.value);
let request = 0;
let disposed = false;
onScopeDispose(() => {
  disposed = true;
  request++;
});
watch(
  () => props.visible,
  (visible) => {
    if (visible) void load();
  },
  { immediate: true },
);
async function load() {
  const sequence = ++request;
  const id = props.provider.identity.id;
  loading.value = true;
  error.value = "";
  try {
    const content = await getOAuthCredential(id);
    if (
      disposed ||
      sequence !== request ||
      props.provider.identity.id !== id
    )
      return;
    original.value = content;
    draft.value = content;
  } catch (cause) {
    if (!disposed && sequence === request) error.value = String(cause);
  } finally {
    if (sequence === request) loading.value = false;
  }
}
function reset() {
  draft.value = original.value;
  error.value = "";
}
async function save() {
  if (saving.value || loading.value || !changed.value) return;
  const id = props.provider.identity.id;
  const current = revision.value;
  saving.value = true;
  try {
    if (
      !(await confirmAction(
        "保存授权原文",
        "这会用当前编辑内容替换本地保存的访问令牌与刷新令牌，并清除待确认的续期状态。若两个应用同时续期同一授权，可能使授权失效。",
        "保存并替换",
        "warning",
      ))
    )
      return;
    if (disposed || props.provider.identity.id !== id) return;
    const provider = await replaceOAuthCredential(id, draft.value, current);
    if (disposed) return;
    original.value = draft.value;
    emit("saved", provider);
    Message.success("授权原文已保存");
    emit("close");
  } catch (cause) {
    if (!disposed) error.value = String(cause);
  } finally {
    saving.value = false;
  }
}
</script>
<template>
  <FileEditorModal
    :visible="visible"
    :title="`授权原文 · ${provider.identity.name}`"
    fill
    @close="emit('close')"
  >
    <div class="oauth-credential-editor">
      <p class="oauth-credential-note">
        这里是 BalanceHub 实际保存的授权内容；修改会直接替换本地令牌，不会改写远端 CPA
        或其他应用。
      </p>
      <a-alert v-if="error" type="error">{{ error }}</a-alert>
      <a-spin v-if="loading" tip="读取授权原文" />
      <ContentEditor
        v-else
        v-model="draft"
        :original-text="original"
        format="json"
        :readonly="saving"
        label="授权原文"
      />
      <footer class="oauth-credential-actions">
        <a-button :disabled="saving" @click="emit('close')">取消</a-button
        ><a-button :disabled="!changed || saving || loading" @click="reset"
          >还原修改</a-button
        ><a-button type="primary" :loading="saving" @click="save"
          >保存授权原文</a-button
        >
      </footer>
    </div>
  </FileEditorModal>
</template>
<style scoped>
.oauth-credential-editor {
  display: flex;
  flex-direction: column;
  gap: 12px;
  height: calc(100vh - 150px);
  min-height: 320px;
}
.oauth-credential-note {
  margin: 0;
  color: var(--color-text-3);
  font-size: 12px;
  line-height: 1.7;
}
.oauth-credential-editor :deep(.content-editor) {
  flex: 1;
  min-height: 0;
}
.oauth-credential-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
</style>
