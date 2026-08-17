<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import { IconFile } from "@arco-design/web-vue/es/icon";
import { useProviderStore, type CliConfigFile, type CliConfigPreview } from "../stores/providers";
import { agentCliLabel } from "../utils/cli-environment";
import AgentCliIcon from "./AgentCliIcon.vue";

type DiffLineKind = "context" | "removed" | "added";

interface DiffLine {
  id: string;
  kind: DiffLineKind;
  text: string;
  oldLine: number | null;
  newLine: number | null;
}

interface DiffFileState {
  lines: DiffLine[];
  trailingNewline: boolean;
}

const MAX_DIFF_MATRIX_CELLS = 250_000;

const props = defineProps<{
  visible: boolean;
  preview: CliConfigPreview | null;
  confirming: boolean;
}>();

const emit = defineEmits<{
  "update:visible": [visible: boolean];
  confirm: [files: CliConfigFile[]];
}>();
const store = useProviderStore();

const cliLabel = computed(() =>
  props.preview ? agentCliLabel(store.cliEnvironmentProbe, props.preview.cliKind) : "Agent CLI",
);

const editableFiles = ref<CliConfigFile[]>([]);
const diffFiles = ref<DiffFileState[]>([]);
const activeFileIndex = ref(0);
const activeFile = computed(() => editableFiles.value[activeFileIndex.value] ?? null);
const activeDiff = computed(() => diffFiles.value[activeFileIndex.value] ?? null);
const activeDiffLines = computed(() => activeDiff.value?.lines ?? []);
const activeChangedLines = computed(
  () => activeDiffLines.value.filter((line) => line.kind !== "context").length,
);
const lineElements = new Map<string, HTMLElement>();

watch(
  () => props.preview,
  (preview) => {
    editableFiles.value = preview?.files.map((file) => ({ ...file })) ?? [];
    const originalFiles = new Map(
      (preview?.originalFiles ?? []).map((file) => [file.filePath, file]),
    );
    diffFiles.value = editableFiles.value.map((file) =>
      buildDiff(originalFiles.get(file.filePath)?.content ?? "", file.content),
    );
    activeFileIndex.value = 0;
    lineElements.clear();
  },
  { immediate: true },
);

function fileName(path: string) {
  return path.split(/[\\/]/).pop() || path;
}

function splitLines(content: string) {
  const normalized = content.replace(/\r\n/g, "\n");
  const trailingNewline = normalized.endsWith("\n");
  const lines = normalized.split("\n");
  if (trailingNewline) {
    lines.pop();
  }
  return { lines: lines.length === 1 && lines[0] === "" ? [] : lines, trailingNewline };
}

function buildDiff(original: string, target: string): DiffFileState {
  const before = splitLines(original).lines;
  const after = splitLines(target);
  if (before.length * after.lines.length > MAX_DIFF_MATRIX_CELLS) {
    return buildCoarseDiff(before, after);
  }
  const matrix = Array.from({ length: before.length + 1 }, () =>
    new Array<number>(after.lines.length + 1).fill(0),
  );

  for (let oldIndex = before.length - 1; oldIndex >= 0; oldIndex -= 1) {
    for (let newIndex = after.lines.length - 1; newIndex >= 0; newIndex -= 1) {
      matrix[oldIndex][newIndex] =
        before[oldIndex] === after.lines[newIndex]
          ? matrix[oldIndex + 1][newIndex + 1] + 1
          : Math.max(matrix[oldIndex + 1][newIndex], matrix[oldIndex][newIndex + 1]);
    }
  }

  const lines: DiffLine[] = [];
  let oldIndex = 0;
  let newIndex = 0;
  let sequence = 0;
  while (oldIndex < before.length || newIndex < after.lines.length) {
    if (
      oldIndex < before.length &&
      newIndex < after.lines.length &&
      before[oldIndex] === after.lines[newIndex]
    ) {
      lines.push({
        id: `context-${oldIndex + 1}-${newIndex + 1}-${sequence++}`,
        kind: "context",
        text: after.lines[newIndex],
        oldLine: oldIndex + 1,
        newLine: newIndex + 1,
      });
      oldIndex += 1;
      newIndex += 1;
    } else if (oldIndex < before.length && shouldRemoveLine(before, after.lines, matrix, oldIndex, newIndex)) {
      lines.push({
        id: `removed-${oldIndex + 1}-${sequence++}`,
        kind: "removed",
        text: before[oldIndex],
        oldLine: oldIndex + 1,
        newLine: null,
      });
      oldIndex += 1;
    } else {
      lines.push({
        id: `added-${newIndex + 1}-${sequence++}`,
        kind: "added",
        text: after.lines[newIndex],
        oldLine: null,
        newLine: newIndex + 1,
      });
      newIndex += 1;
    }
  }

  return { lines, trailingNewline: after.trailingNewline };
}

function shouldRemoveLine(
  before: string[],
  after: string[],
  matrix: number[][],
  oldIndex: number,
  newIndex: number,
) {
  if (newIndex >= after.length || matrix[oldIndex + 1][newIndex] > matrix[oldIndex][newIndex + 1]) {
    return true;
  }
  if (matrix[oldIndex + 1][newIndex] < matrix[oldIndex][newIndex + 1]) {
    return false;
  }
  const oldLineAppearsLater = after.indexOf(before[oldIndex], newIndex + 1) !== -1;
  const newLineAppearsLater = before.indexOf(after[newIndex], oldIndex + 1) !== -1;
  return !(oldLineAppearsLater && !newLineAppearsLater);
}

function buildCoarseDiff(
  before: string[],
  after: { lines: string[]; trailingNewline: boolean },
): DiffFileState {
  let prefix = 0;
  while (prefix < before.length && prefix < after.lines.length && before[prefix] === after.lines[prefix]) {
    prefix += 1;
  }
  let suffix = 0;
  while (
    suffix < before.length - prefix &&
    suffix < after.lines.length - prefix &&
    before[before.length - suffix - 1] === after.lines[after.lines.length - suffix - 1]
  ) {
    suffix += 1;
  }

  const lines: DiffLine[] = [];
  let sequence = 0;
  for (let index = 0; index < prefix; index += 1) {
    lines.push({
      id: `context-${index + 1}-${index + 1}-${sequence++}`,
      kind: "context",
      text: before[index],
      oldLine: index + 1,
      newLine: index + 1,
    });
  }
  for (let index = prefix; index < before.length - suffix; index += 1) {
    lines.push({
      id: `removed-${index + 1}-${sequence++}`,
      kind: "removed",
      text: before[index],
      oldLine: index + 1,
      newLine: null,
    });
  }
  for (let index = prefix; index < after.lines.length - suffix; index += 1) {
    lines.push({
      id: `added-${index + 1}-${sequence++}`,
      kind: "added",
      text: after.lines[index],
      oldLine: null,
      newLine: index + 1,
    });
  }
  for (let index = 0; index < suffix; index += 1) {
    const oldLine = before.length - suffix + index;
    const newLine = after.lines.length - suffix + index;
    lines.push({
      id: `context-${oldLine + 1}-${newLine + 1}-${sequence++}`,
      kind: "context",
      text: before[oldLine],
      oldLine: oldLine + 1,
      newLine: newLine + 1,
    });
  }
  return { lines, trailingNewline: after.trailingNewline };
}

function syncActiveFile() {
  const file = activeFile.value;
  const state = activeDiff.value;
  if (!file || !state) {
    return;
  }
  const content = state.lines
    .filter((line) => line.kind !== "removed")
    .map((line) => line.text)
    .join("\n");
  file.content = state.trailingNewline ? `${content}\n` : content;
}

function setLineElement(line: DiffLine, element: unknown) {
  if (element instanceof HTMLElement) {
    lineElements.set(line.id, element);
  } else {
    lineElements.delete(line.id);
  }
}

function focusLine(lineId: string, offset?: number) {
  void nextTick(() => {
    const element = lineElements.get(lineId);
    if (!element) {
      return;
    }
    element.focus();
    const selection = window.getSelection();
    const range = document.createRange();
    range.selectNodeContents(element);
    range.collapse(true);
    if (offset !== undefined && element.firstChild) {
      range.setStart(element.firstChild, Math.min(offset, element.textContent?.length ?? 0));
      range.collapse(true);
    }
    selection?.removeAllRanges();
    selection?.addRange(range);
  });
}

function promoteContextLine(index: number) {
  const state = activeDiff.value;
  const line = state?.lines[index];
  if (!state || !line || line.kind !== "context") {
    return line;
  }
  const originalLine: DiffLine = {
    id: `${line.id}-removed`,
    kind: "removed",
    text: line.text,
    oldLine: line.oldLine,
    newLine: null,
  };
  line.kind = "added";
  line.oldLine = null;
  state.lines.splice(index, 0, originalLine);
  return line;
}

function updateDiffLine(index: number, event: Event) {
  const state = activeDiff.value;
  const line = state?.lines[index];
  const element = event.currentTarget;
  if (!line || !(element instanceof HTMLElement)) {
    return;
  }
  const nextText = (element.textContent ?? "").replace(/[\r\n]/g, "");
  const originalId = line.id;
  if (line.kind === "context" && nextText !== line.text) {
    const promoted = promoteContextLine(index);
    if (promoted) {
      promoted.text = nextText;
      lineElements.set(promoted.id, element);
    }
  } else {
    line.text = nextText;
  }
  if (originalId !== line.id) {
    lineElements.delete(originalId);
  }
  syncActiveFile();
}

function cursorOffset(element: HTMLElement) {
  const selection = window.getSelection();
  if (!selection || selection.rangeCount === 0 || !selection.anchorNode) {
    return element.textContent?.length ?? 0;
  }
  const range = document.createRange();
  range.selectNodeContents(element);
  range.setEnd(selection.anchorNode, selection.anchorOffset);
  return range.toString().length;
}

function insertDiffLine(index: number, event: KeyboardEvent) {
  event.preventDefault();
  const state = activeDiff.value;
  const line = state?.lines[index];
  const element = event.currentTarget;
  if (!state || !line || !(element instanceof HTMLElement)) {
    return;
  }
  const offset = cursorOffset(element);
  const editableLine = promoteContextLine(index) ?? line;
  const text = editableLine.text;
  editableLine.text = text.slice(0, offset);
  const newLine: DiffLine = {
    id: `added-${Date.now()}-${Math.random().toString(36).slice(2)}`,
    kind: "added",
    text: text.slice(offset),
    oldLine: null,
    newLine: null,
  };
  const editableIndex = state.lines.indexOf(editableLine);
  state.lines.splice(editableIndex + 1, 0, newLine);
  syncActiveFile();
  focusLine(newLine.id);
}

function mergeLine(index: number, event: KeyboardEvent, direction: "previous" | "next") {
  const state = activeDiff.value;
  const line = state?.lines[index];
  const element = event.currentTarget;
  if (!state || !line || !(element instanceof HTMLElement)) {
    return;
  }
  const atStart = cursorOffset(element) === 0;
  const atEnd = cursorOffset(element) === (element.textContent?.length ?? 0);
  const otherIndex = direction === "previous" ? index - 1 : index + 1;
  const other = state.lines[otherIndex];
  if (!other || other.kind === "removed") {
    return;
  }
  if ((direction === "previous" && !atStart) || (direction === "next" && !atEnd)) {
    return;
  }
  event.preventDefault();
  if (direction === "previous") {
    const mergedText = `${other.text}${line.text}`;
    other.text = mergedText;
    state.lines.splice(index, 1);
    syncActiveFile();
    focusLine(other.id, other.text.length - line.text.length);
  } else {
    line.text = `${line.text}${other.text}`;
    state.lines.splice(otherIndex, 1);
    syncActiveFile();
    focusLine(line.id, line.text.length - other.text.length);
  }
}

function pasteDiffLine(index: number, event: ClipboardEvent) {
  const pasted = event.clipboardData?.getData("text/plain") ?? "";
  if (!pasted.includes("\n")) {
    return;
  }
  event.preventDefault();
  const state = activeDiff.value;
  const line = state?.lines[index];
  const element = event.currentTarget;
  if (!state || !line || !(element instanceof HTMLElement)) {
    return;
  }
  const offset = cursorOffset(element);
  const editableLine = promoteContextLine(index) ?? line;
  const before = editableLine.text.slice(0, offset);
  const after = editableLine.text.slice(offset);
  const pastedLines = pasted.replace(/\r\n/g, "\n").split("\n");
  editableLine.text = `${before}${pastedLines.shift() ?? ""}`;
  const inserted = pastedLines.map((text, insertedIndex) => ({
    id: `added-${Date.now()}-${insertedIndex}-${Math.random().toString(36).slice(2)}`,
    kind: "added" as const,
    text,
    oldLine: null,
    newLine: null,
  }));
  inserted[inserted.length - 1].text += after;
  const editableIndex = state.lines.indexOf(editableLine);
  state.lines.splice(editableIndex + 1, 0, ...inserted);
  syncActiveFile();
  focusLine(inserted[inserted.length - 1]?.id ?? editableLine.id);
}

function lineMarker(kind: DiffLineKind) {
  return kind === "removed" ? "-" : kind === "added" ? "+" : " ";
}

function confirm() {
  emit(
    "confirm",
    editableFiles.value.map((file) => ({ ...file })),
  );
}
</script>

<template>
  <a-modal
    :visible="visible"
    width="min(1000px, calc(100vw - 32px))"
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
        <span class="surface-modal-title-icon">
          <AgentCliIcon v-if="preview" :kind="preview.cliKind" :size="18" />
          <IconFile v-else />
        </span>
        <span class="surface-modal-title-copy">
          <strong>编辑 {{ cliLabel }} 默认配置</strong>
        </span>
        <span v-if="preview" class="surface-modal-title-meta">{{ preview.files.length }} 个文件</span>
      </div>
    </template>

    <div v-if="preview" class="cli-config-preview">
      <header class="cli-config-preview-summary">
        <div class="cli-config-preview-summary-target">
          <span>目标中转站</span>
          <strong>{{ preview.providerName }}</strong>
        </div>
      </header>

      <div class="cli-config-editor">
        <nav class="cli-config-file-tabs" aria-label="配置文件">
          <button
            v-for="(file, index) in editableFiles"
            :key="file.filePath"
            type="button"
            :class="{ active: index === activeFileIndex }"
            @click="activeFileIndex = index"
          >
            <icon-file aria-hidden="true" />
            <span :title="file.filePath">{{ fileName(file.filePath) }}</span>
          </button>
        </nav>

        <section v-if="activeFile && activeDiff" class="cli-config-file-editor">
          <header>
            <code :title="activeFile.filePath">{{ activeFile.filePath }}</code>
            <span>{{ activeChangedLines > 0 ? `${activeChangedLines} 处变更` : "无变更" }}</span>
          </header>
          <div class="cli-config-file-editor-body">
            <div class="cli-config-inline-diff" aria-label="配置文件行级差异编辑器">
              <div class="cli-config-inline-diff-scroll">
                <div
                  v-for="(line, index) in activeDiffLines"
                  :key="line.id"
                  class="cli-config-inline-diff-line"
                  :class="`cli-config-inline-diff-line-${line.kind}`"
                >
                  <span class="cli-config-inline-diff-marker" aria-hidden="true">{{ lineMarker(line.kind) }}</span>
                  <span class="cli-config-inline-diff-number">{{ line.oldLine ?? "" }}</span>
                  <span class="cli-config-inline-diff-number">{{ line.newLine ?? "" }}</span>
                  <div
                    v-if="line.kind !== 'removed'"
                    :ref="(element) => setLineElement(line, element)"
                    class="cli-config-inline-diff-text cli-config-inline-diff-text-editable"
                    contenteditable="true"
                    role="textbox"
                    spellcheck="false"
                    @input="updateDiffLine(index, $event)"
                    @keydown.enter="insertDiffLine(index, $event)"
                    @keydown.backspace="mergeLine(index, $event, 'previous')"
                    @keydown.delete="mergeLine(index, $event, 'next')"
                    @paste="pasteDiffLine(index, $event)"
                  >{{ line.text }}</div>
                  <code v-else class="cli-config-inline-diff-text">{{ line.text }}</code>
                </div>
              </div>
            </div>
          </div>
        </section>
      </div>
    </div>

    <template #footer>
      <a-button
        type="primary"
        :loading="confirming"
        :disabled="!preview || editableFiles.length === 0"
        @click="confirm"
      >
        保存完整配置
      </a-button>
    </template>
  </a-modal>
</template>
