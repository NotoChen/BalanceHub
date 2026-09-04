<script setup lang="ts">
import { computed } from "vue";
import { IconCopy, IconFile, IconFolder, IconLoading } from "@arco-design/web-vue/es/icon";
import type { AgentAssetReadResult, AgentAssetRecord } from "../../../stores/provider-types";

const props = defineProps<{
  files: AgentAssetRecord[];
  supported: boolean;
  selectedId: string | null;
  preview: AgentAssetReadResult | null;
  previewState: "idle" | "loading" | "refreshing" | "ready" | "error";
  previewError: string;
  copiedPathId: string | null;
}>();
const emit = defineEmits<{ select: [asset: AgentAssetRecord]; open: [asset: AgentAssetRecord]; copy: [asset: AgentAssetRecord] }>();

const selectedFile = computed(() => props.files.find((file) => file.stableId === props.selectedId) ?? null);

function emitSelected(action: "open" | "copy") {
  if (!selectedFile.value) return;
  if (action === "open") emit("open", selectedFile.value);
  else emit("copy", selectedFile.value);
}
</script>

<template>
  <div class="agent-config-browser">
    <div class="agent-config-file-list">
      <button
        v-for="file in files"
        :key="file.stableId"
        type="button"
        class="agent-config-file"
        :class="{ active: file.stableId === selectedId }"
        @click="$emit('select', file)"
      >
        <IconFile />
        <span>
          <strong>{{ file.label || file.nativeId }}</strong>
          <small>{{ file.path || "路径不可用" }}</small>
        </span>
      </button>
      <div v-if="!supported" class="agent-environment-empty">此安装不支持配置盘点</div>
      <div v-else-if="files.length === 0" class="agent-environment-empty">未发现可预览的配置文件</div>
    </div>
    <div class="agent-config-preview">
      <div v-if="previewState === 'loading' && !preview" class="agent-config-preview-empty"><IconLoading class="agent-version-loading" /> 正在读取配置</div>
      <div v-else-if="previewState === 'error' && !preview" class="agent-config-preview-empty is-error">{{ previewError }}</div>
      <div v-else-if="!preview" class="agent-config-preview-empty">选择配置文件查看只读预览</div>
      <template v-else>
        <header class="agent-config-preview-head">
          <span :title="preview.path">{{ preview.path }}</span>
          <span>
            <IconLoading v-if="previewState === 'refreshing'" class="agent-version-loading" />
            {{ preview.metadataOnly ? "仅元数据" : preview.truncated ? "内容已截断" : `${preview.sizeBytes} B` }}
          </span>
        </header>
        <pre v-if="preview.content !== null" class="agent-config-content">{{ preview.content }}</pre>
        <div v-else class="agent-config-preview-empty">
          {{ preview.metadataOnly ? "当前资产仅提供元数据" : preview.diagnostic ? "文件缺失或不可读取" : "当前文件不提供内容预览" }}
        </div>
        <p v-if="preview.diagnostic" class="agent-config-diagnostic">{{ preview.diagnostic }}</p>
        <p v-if="previewError" class="agent-environment-stale-error agent-config-stale-error">
          刷新失败，保留上次成功结果：{{ previewError }}
        </p>
        <div class="agent-config-actions">
          <a-button size="small" :disabled="!selectedFile" @click="emitSelected('copy')">
            <template #icon><IconCopy /></template>复制路径
          </a-button>
          <a-button size="small" :disabled="!selectedFile" @click="emitSelected('open')">
            <template #icon><IconFolder /></template>打开文件
          </a-button>
          <span v-if="selectedId && copiedPathId === selectedId" class="agent-asset-copied">已复制</span>
        </div>
      </template>
    </div>
  </div>
</template>
