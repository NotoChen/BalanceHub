<script setup lang="ts">
import { computed, onScopeDispose, ref, watch } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import {
  IconCheck,
  IconDelete,
  IconFolder,
  IconFolderAdd,
  IconHome,
  IconRefresh,
  IconRight,
  IconUp,
} from "@arco-design/web-vue/es/icon";
import type {
  Workspace,
  WorkspaceDirectoryListing,
} from "../stores/provider-types";
import { withTimeout } from "../utils/promise-timeout";

const props = defineProps<{
  active: boolean;
  workspaces: Workspace[];
  directory: WorkspaceDirectoryListing | null;
  pathDraft: string;
  browsing: boolean;
  forgettingPath: string | null;
  disabled?: boolean;
}>();
const emit = defineEmits<{
  "update:pathDraft": [path: string];
  browse: [path?: string];
  forget: [path: string];
}>();
const pathModel = computed({
  get: () => props.pathDraft,
  set: (path: string) => emit("update:pathDraft", path),
});
const showHidden = ref(false);
const choosing = ref(false);
const error = ref("");
let chooserRevision = 0;
const ordered = computed(() =>
  [...props.workspaces].sort(
    (a, b) => b.useCount - a.useCount || a.path.localeCompare(b.path),
  ),
);
const entries = computed(() =>
  (props.directory?.entries ?? []).filter(
    (entry) => showHidden.value || !entry.hidden,
  ),
);
const repeatedNames = computed(() => {
  const counts = new Map<string, number>();
  for (const workspace of ordered.value) {
    const label = name(workspace.path);
    counts.set(label, (counts.get(label) ?? 0) + 1);
  }
  return new Set(
    [...counts].filter(([, count]) => count > 1).map(([label]) => label),
  );
});
function name(path: string) {
  return (
    path
      .replace(/[\\/]+$/, "")
      .split(/[\\/]/)
      .pop() || path
  );
}
function browsePath() {
  if (!props.browsing && pathModel.value.trim())
    emit("browse", pathModel.value.trim());
}
function invalidate() {
  chooserRevision++;
  choosing.value = false;
}
watch(
  () => props.active,
  () => {
    invalidate();
    error.value = "";
  },
);
watch(() => props.directory?.currentPath, invalidate);
onScopeDispose(invalidate);
async function chooseDirectory() {
  if (choosing.value || !props.active) return;
  const revision = ++chooserRevision;
  choosing.value = true;
  error.value = "";
  try {
    const path = await withTimeout(
      open({
        directory: true,
        multiple: false,
        title: "选择本次 CLI 的工作目录",
        defaultPath: props.directory?.currentPath,
      }),
      5 * 60_000,
      "目录选择已超时，请重新打开",
    );
    if (
      revision === chooserRevision &&
      props.active &&
      typeof path === "string"
    )
      emit("browse", path);
  } catch (cause) {
    if (revision === chooserRevision) error.value = String(cause);
  } finally {
    if (revision === chooserRevision) choosing.value = false;
  }
}
</script>

<template>
  <div class="cli-directory-panel">
    <section class="cli-directory-favorites">
      <header class="cli-directory-section-heading">
        <strong>常用目录</strong><span>{{ ordered.length }}</span>
      </header>
      <div v-if="ordered.length" class="cli-directory-favorite-list">
        <div
          v-for="workspace in ordered"
          :key="workspace.path"
          class="cli-directory-favorite"
          :class="{ selected: directory?.currentPath === workspace.path }"
        >
          <button
            type="button"
            :title="workspace.path"
            :aria-label="workspace.path"
            :disabled="disabled || browsing"
            @click="emit('browse', workspace.path)"
          >
            <icon-folder /><span
              ><strong>{{ name(workspace.path) }}</strong
              ><small v-if="repeatedNames.has(name(workspace.path))">{{
                workspace.path
              }}</small></span
            ><icon-check v-if="directory?.currentPath === workspace.path" />
          </button>
          <a-tooltip content="移出常用目录"
            ><a-button
              type="text"
              size="mini"
              class="cli-directory-remove"
              :disabled="disabled"
              :loading="forgettingPath === workspace.path"
              aria-label="移出常用目录"
              @click="emit('forget', workspace.path)"
              ><template #icon><icon-delete /></template></a-button
          ></a-tooltip>
        </div>
      </div>
      <p v-else class="cli-directory-empty">使用过的工作目录会显示在这里</p>
    </section>
    <section class="cli-directory-browser">
      <header class="cli-directory-section-heading">
        <strong>工作目录</strong
        ><a-button
          size="small"
          type="text"
          :disabled="disabled"
          :loading="choosing"
          @click="chooseDirectory"
          ><template #icon><icon-folder-add /></template>选择目录</a-button
        >
      </header>
      <div class="cli-directory-path">
        <a-input
          v-model="pathModel"
          size="small"
          :disabled="disabled"
          placeholder="输入目录路径"
          aria-label="工作目录路径"
          @press-enter="browsePath"
        /><a-button
          size="small"
          :disabled="disabled || browsing || !pathModel.trim()"
          aria-label="打开输入的目录"
          @click="browsePath"
          ><template #icon><icon-right /></template
        ></a-button>
      </div>
      <div class="cli-directory-toolbar">
        <div>
          <a-tooltip content="主目录"
            ><a-button
              type="text"
              size="mini"
              :disabled="disabled || browsing || !directory"
              aria-label="主目录"
              @click="emit('browse', directory?.homePath)"
              ><template #icon><icon-home /></template></a-button
          ></a-tooltip>
          <a-tooltip content="上级目录"
            ><a-button
              type="text"
              size="mini"
              :disabled="disabled || browsing || !directory?.parentPath"
              aria-label="上级目录"
              @click="emit('browse', directory?.parentPath ?? undefined)"
              ><template #icon><icon-up /></template></a-button
          ></a-tooltip>
          <a-tooltip content="刷新目录"
            ><a-button
              type="text"
              size="mini"
              :disabled="disabled || !directory"
              :loading="browsing"
              aria-label="刷新目录"
              @click="emit('browse', directory?.currentPath)"
              ><template #icon><icon-refresh /></template></a-button
          ></a-tooltip>
        </div>
        <a-checkbox v-model="showHidden" :disabled="disabled"
          >显示隐藏项</a-checkbox
        >
      </div>
      <p v-if="error" class="cli-run-error" role="alert">{{ error }}</p>
      <div class="cli-directory-list" :aria-busy="browsing">
        <button
          v-for="entry in entries"
          :key="entry.path"
          type="button"
          :title="entry.path"
          :disabled="disabled || browsing"
          @click="emit('browse', entry.path)"
        >
          <icon-folder /><span>{{ entry.name }}</span
          ><icon-right />
        </button>
        <p v-if="!entries.length" class="cli-directory-empty" role="status">
          {{
            browsing
              ? "正在读取目录…"
              : directory
                ? "当前目录没有子目录"
                : "选择本次会话的工作目录"
          }}
        </p>
      </div>
    </section>
  </div>
</template>
