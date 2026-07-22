<script setup lang="ts">
import { computed } from "vue";
import { IconFile, IconLock } from "@arco-design/web-vue/es/icon";
import type { CliConfigChange, CliConfigPreview } from "../stores/providers";
import BrandIcon from "./BrandIcon.vue";

const props = defineProps<{
  visible: boolean;
  preview: CliConfigPreview | null;
  confirming: boolean;
}>();

const emit = defineEmits<{
  "update:visible": [visible: boolean];
  confirm: [];
}>();

const cliLabel = computed(() =>
  props.preview?.cliKind === "claudeCode" ? "Claude Code" : "Codex",
);

const cliBrand = computed(() =>
  props.preview?.cliKind === "claudeCode" ? "claude" : "codex",
);

const groupedChanges = computed(() => {
  const groups = new Map<string, CliConfigChange[]>();
  for (const change of props.preview?.changes ?? []) {
    const current = groups.get(change.filePath) ?? [];
    current.push(change);
    groups.set(change.filePath, current);
  }
  return [...groups.entries()].map(([filePath, changes]) => ({ filePath, changes }));
});

function displayValue(value: string | null, sensitive: boolean) {
  if (value === null || value === "") {
    return "(未设置)";
  }
  if (!sensitive) {
    return value;
  }
  if (value.length <= 12) {
    return "••••••••";
  }
  return `${value.slice(0, 6)}••••${value.slice(-4)}`;
}
</script>

<template>
  <a-modal
    :visible="visible"
    width="min(760px, calc(100vw - 32px))"
    modal-class="surface-modal cli-config-preview-modal"
    title-align="start"
    :closable="!confirming"
    :mask-closable="!confirming"
    :esc-to-close="!confirming"
    unmount-on-close
    @update:visible="emit('update:visible', $event)"
  >
    <template #title>
      <div class="surface-modal-title cli-config-preview-title">
        <span class="surface-modal-title-icon"><BrandIcon :brand="cliBrand" :size="18" /></span>
        <span class="surface-modal-title-copy">
          <strong>切换 {{ cliLabel }} 默认配置</strong>
        </span>
        <span v-if="preview" class="surface-modal-title-meta">{{ preview.changes.length }} 处变更</span>
      </div>
    </template>

    <div v-if="preview" class="cli-config-preview">
      <header class="cli-config-preview-summary">
        <span>目标中转站</span>
        <strong>{{ preview.providerName }}</strong>
      </header>

      <div v-if="groupedChanges.length > 0" class="cli-config-preview-files">
        <section
          v-for="group in groupedChanges"
          :key="group.filePath"
          class="cli-config-preview-file"
        >
          <header>
            <icon-file aria-hidden="true" />
            <code :title="group.filePath">{{ group.filePath }}</code>
          </header>
          <div class="cli-config-preview-changes">
            <article
              v-for="change in group.changes"
              :key="change.fieldPath"
              class="cli-config-preview-change"
            >
              <div class="cli-config-preview-field">
                <code>{{ change.fieldPath }}</code>
                <span v-if="change.sensitive" title="敏感值已隐藏">
                  <icon-lock aria-hidden="true" />
                  敏感字段
                </span>
              </div>
              <div class="cli-config-diff-line cli-config-diff-before">
                <b aria-hidden="true">−</b>
                <code>{{ displayValue(change.beforeValue, change.sensitive) }}</code>
              </div>
              <div class="cli-config-diff-line cli-config-diff-after">
                <b aria-hidden="true">+</b>
                <code>{{ displayValue(change.afterValue, change.sensitive) }}</code>
              </div>
            </article>
          </div>
        </section>
      </div>

      <a-empty v-else description="当前配置已经一致，无需写入" />
    </div>

    <template #footer>
      <a-button :disabled="confirming" @click="emit('update:visible', false)">取消</a-button>
      <a-button
        type="primary"
        :loading="confirming"
        :disabled="!preview || preview.changes.length === 0"
        @click="emit('confirm')"
      >
        确认写入
      </a-button>
    </template>
  </a-modal>
</template>
