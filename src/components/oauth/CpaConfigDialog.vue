<script setup lang="ts">
import { computed, onScopeDispose, ref, watch } from "vue";
import { Message } from "@arco-design/web-vue";
import type { Provider } from "../../stores/provider-types";
import type { CpaGatewayConfig } from "../../stores/oauth-types";
import {
  readCpaGatewayConfig,
  updateCpaGatewayConfig,
} from "../../api/oauth-accounts";
import ContentEditor from "../ContentEditor.vue";

const props = defineProps<{
  visible: boolean;
  provider: Provider;
  revision: number;
}>();
const emit = defineEmits<{ close: [] }>();
const loading = ref(false);
const saving = ref(false);
const error = ref("");
const config = ref<CpaGatewayConfig | null>(null);
const draft = ref("");
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
const dirty = computed(() => {
  const saved = config.value;
  return !!saved && draft.value !== saved.content;
});
/// The document is what the gateway holds, so a failed read leaves nothing to
/// edit rather than an empty editor that would replace the file on save.
async function load() {
  loading.value = true;
  error.value = "";
  config.value = null;
  draft.value = "";
  const current = ++sequence;
  try {
    const result = await readCpaGatewayConfig(props.provider.identity.id);
    if (!disposed && current === sequence) {
      config.value = result;
      draft.value = result.content;
    }
  } catch (cause) {
    if (!disposed && current === sequence) error.value = String(cause);
  } finally {
    if (current === sequence) loading.value = false;
  }
}
async function save() {
  const saved = config.value;
  if (!saved || saving.value || !dirty.value) return;
  saving.value = true;
  error.value = "";
  try {
    await updateCpaGatewayConfig(
      props.provider.identity.id,
      draft.value,
      saved.etag,
      saved.apiVersion,
      props.revision,
    );
    if (disposed) return;
    Message.success("网关配置已保存");
    /// The gateway rewrites the document in its own layout and issues a new
    /// revision tag, so the editor reloads what it now holds instead of
    /// keeping text that is no longer the file.
    if (props.visible) await load();
  } catch (cause) {
    if (!disposed) error.value = String(cause);
  } finally {
    saving.value = false;
  }
}
function discard() {
  if (config.value) draft.value = config.value.content;
  error.value = "";
}
function reset() {
  sequence++;
  config.value = null;
  draft.value = "";
  error.value = "";
  loading.value = false;
  saving.value = false;
}
</script>

<template>
  <a-modal
    :visible="visible"
    title="网关配置原文"
    width="880px"
    :footer="false"
    unmount-on-close
    @cancel="emit('close')"
  >
    <a-alert v-if="error" type="warning">{{ error }}</a-alert>
    <a-spin v-if="loading" tip="读取网关配置" />
    <template v-else-if="config">
      <p class="cpa-config-hint">
        这里编辑网关自己的 config.yaml，可修改调度与重试之外的值。保存时网关会先校验，校验不通过就保留原配置；v8 网关还会核对读取时的版本标记，其他会话改过时保存会被拒绝。配置里可能含有密钥，内容只在本机与网关之间传输。
      </p>
      <ContentEditor
        v-model="draft"
        :original-text="config.content"
        format="yaml"
        label="CPA 网关配置"
      />
      <div class="cpa-config-actions">
        <span v-if="dirty" class="cpa-config-dirty">有未保存的修改</span
        ><a-button size="mini" :disabled="!dirty || saving" @click="discard"
          >撤销</a-button
        ><a-button
          size="mini"
          type="primary"
          :disabled="!dirty"
          :loading="saving"
          @click="save"
          >保存配置</a-button
        >
      </div>
    </template>
    <p v-else-if="!error" class="cpa-config-empty">未能读取网关配置。</p>
  </a-modal>
</template>

<style scoped>
.cpa-config-hint {
  margin: 0 0 10px;
  color: var(--color-text-3);
  font-size: 11px;
  line-height: 1.65;
}
.cpa-config-actions {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 10px;
}
.cpa-config-dirty {
  margin-right: auto;
  color: #b26a00;
  font-size: 11px;
}
.cpa-config-empty {
  margin: 0;
  color: var(--color-text-3);
  font-size: 12px;
}
</style>
