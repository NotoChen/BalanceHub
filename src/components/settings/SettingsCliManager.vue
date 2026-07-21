<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Message } from "@arco-design/web-vue";
import {
  useProviderStore,
  type AppSettings,
  type CliCandidate,
  type LivenessCliKind,
} from "../../stores/providers";
import BrandIcon, { type BrandIconName } from "../BrandIcon.vue";

const props = defineProps<{
  settings: AppSettings;
}>();

const store = useProviderStore();

interface CliCardState {
  status: "idle" | "checking" | "ok" | "error";
  version: string;
  resolvedPath: string;
  message: string;
}

interface CandidatesState {
  items: CliCandidate[];
  loading: boolean;
  expanded: boolean;
  scanned: boolean;
  message: string;
  lastScannedAt: string;
}

const CLI_KINDS: LivenessCliKind[] = ["codex", "claudeCode"];
const CLI_CANDIDATES_CACHE_TTL_MS = 10 * 60 * 1000;

interface CliCandidatesCacheEntry {
  items: CliCandidate[];
  preferredPath: string;
  lastScannedAt: string;
  at: number;
}

const cliCandidatesCache: Partial<Record<LivenessCliKind, CliCandidatesCacheEntry>> = {};

const cliMeta: Record<LivenessCliKind, { label: string; install: string; doc: string }> = {
  codex: {
    label: "Codex CLI",
    install: "npm i -g @openai/codex",
    doc: "https://github.com/openai/codex",
  },
  claudeCode: {
    label: "Claude Code CLI",
    install: "npm i -g @anthropic-ai/claude-code",
    doc: "https://docs.anthropic.com/en/docs/claude-code/overview",
  },
};

const state = reactive<Record<LivenessCliKind, CliCardState>>({
  codex: { status: "idle", version: "", resolvedPath: "", message: "" },
  claudeCode: { status: "idle", version: "", resolvedPath: "", message: "" },
});

const candidates = reactive<Record<LivenessCliKind, CandidatesState>>({
  codex: { items: [], loading: false, expanded: false, scanned: false, message: "", lastScannedAt: "" },
  claudeCode: { items: [], loading: false, expanded: false, scanned: false, message: "", lastScannedAt: "" },
});

let primeTimer: ReturnType<typeof setTimeout> | null = null;

const currentCliLabel = computed(() => cliMeta[props.settings.livenessCliKind].label);
const readyCliCount = computed(() => CLI_KINDS.filter((kind) => state[kind].status === "ok").length);

function pathFor(kind: LivenessCliKind) {
  return kind === "claudeCode" ? props.settings.claudeCliPath : props.settings.codexCliPath;
}

function setPath(kind: LivenessCliKind, value: string) {
  if (kind === "claudeCode") {
    props.settings.claudeCliPath = value;
  } else {
    props.settings.codexCliPath = value;
  }
}

function setCurrentKind(kind: LivenessCliKind) {
  props.settings.livenessCliKind = kind;
}

function candidateSummary(kind: LivenessCliKind) {
  const view = candidates[kind];
  const validCount = view.items.filter((item) => item.valid).length;
  if (view.loading) return "扫描中";
  if (!view.scanned) return "未扫描";
  if (view.items.length === 0) return "未发现候选";
  return `${view.items.length} 个候选，${validCount} 个可用`;
}

function statusLabel(kind: LivenessCliKind) {
  const card = state[kind];
  if (card.status === "checking") return "校验中";
  if (card.status === "ok") return card.version || "可用";
  if (card.status === "error") return "未就绪";
  return "未校验";
}

function statusTone(kind: LivenessCliKind) {
  return `cli-overview-${state[kind].status}`;
}

function cliBrand(kind: LivenessCliKind): BrandIconName {
  return kind === "codex" ? "codex" : "claude";
}

function normalizedPath(value: string) {
  return value.trim();
}

function isSelectedCandidate(kind: LivenessCliKind, candidate: CliCandidate) {
  return normalizedPath(pathFor(kind)) === normalizedPath(candidate.path);
}

function defaultCandidate(kind: LivenessCliKind) {
  return candidates[kind].items.find((candidate) => candidate.isPathDefault) ?? null;
}

async function validate(kind: LivenessCliKind) {
  const card = state[kind];
  card.status = "checking";
  card.message = "";
  try {
    const result = await store.checkCliPath(kind, pathFor(kind).trim());
    card.status = "ok";
    card.version = result.version;
    card.resolvedPath = result.path;
    if (normalizedPath(result.path) !== normalizedPath(pathFor(kind))) {
      setPath(kind, result.path);
    }
  } catch (error) {
    card.status = "error";
    card.version = "";
    card.resolvedPath = "";
    card.message = error instanceof Error ? error.message : String(error);
  }
}

async function onPathChange(kind: LivenessCliKind, value: string) {
  setPath(kind, value);
  await validate(kind);
}

async function rescan(kind: LivenessCliKind) {
  if (candidates[kind].loading) {
    return;
  }
  const card = state[kind];
  card.status = "checking";
  card.message = "";
  try {
    await loadCandidates(kind, true);
    const firstValid = candidates[kind].items.find((item) => item.valid);
    if (!firstValid) {
      card.status = "error";
      card.version = "";
      card.resolvedPath = "";
      card.message = `未找到可用的 ${cliMeta[kind].label}`;
      return;
    }
    setPath(kind, firstValid.path);
    card.status = "ok";
    card.version = firstValid.version ?? "";
    card.resolvedPath = firstValid.path;
    Message.success(`已找到 ${cliMeta[kind].label}：${firstValid.version || firstValid.path}`);
  } catch (error) {
    card.status = "error";
    card.version = "";
    card.resolvedPath = "";
    card.message = error instanceof Error ? error.message : String(error);
  }
}

async function browse(kind: LivenessCliKind) {
  try {
    const selected = await open({
      multiple: false,
      directory: false,
      title: `选择 ${cliMeta[kind].label} 可执行文件`,
    });
    if (!selected || Array.isArray(selected)) {
      return;
    }
    setPath(kind, selected);
    await validate(kind);
    await loadCandidates(kind, false);
  } catch (error) {
    Message.error(error instanceof Error ? error.message : String(error));
  }
}

function toggleCandidates(kind: LivenessCliKind) {
  const view = candidates[kind];
  view.expanded = !view.expanded;
}

async function useCandidate(kind: LivenessCliKind, candidate: CliCandidate) {
  if (!candidate.valid) {
    Message.warning("该候选未通过版本校验，不能直接用于测活");
    return;
  }
  setPath(kind, candidate.path);
  await validate(kind);
}

async function loadCandidates(kind: LivenessCliKind, manual: boolean) {
  const view = candidates[kind];
  if (view.loading) {
    return;
  }
  const preferredPath = pathFor(kind).trim();
  const cached = cliCandidatesCache[kind];
  if (
    !manual &&
    cached &&
    cached.preferredPath === preferredPath &&
    Date.now() - cached.at < CLI_CANDIDATES_CACHE_TTL_MS
  ) {
    view.items = cached.items;
    view.scanned = true;
    view.lastScannedAt = cached.lastScannedAt;
    view.message = "";
    return;
  }
  view.loading = true;
  view.message = "";
  try {
    view.items = await store.listCliCandidates(kind, preferredPath);
    view.scanned = true;
    view.lastScannedAt = new Date().toLocaleTimeString("zh-CN", { hour12: false });
    cliCandidatesCache[kind] = {
      items: [...view.items],
      preferredPath,
      lastScannedAt: view.lastScannedAt,
      at: Date.now(),
    };
    if (manual) {
      const validCount = view.items.filter((item) => item.valid).length;
      const summary =
        view.items.length === 0 ? "未发现候选" : `${view.items.length} 个候选，${validCount} 个可用`;
      Message.success(`${cliMeta[kind].label} 候选扫描完成：${summary}`);
    }
  } catch (error) {
    view.message = error instanceof Error ? error.message : String(error);
    if (manual) {
      Message.error(view.message);
    }
  } finally {
    view.loading = false;
  }
}

async function primeCandidates() {
  for (const kind of CLI_KINDS) {
    await loadCandidates(kind, false);
  }
}

async function openInstallDoc(kind: LivenessCliKind) {
  try {
    await openUrl(cliMeta[kind].doc);
  } catch {
    // 打开外部链接失败时静默忽略。
  }
}

onMounted(() => {
  for (const kind of CLI_KINDS) {
    if (pathFor(kind).trim()) {
      void validate(kind);
    }
  }
  primeTimer = setTimeout(() => {
    void primeCandidates();
  }, 200);
});

onUnmounted(() => {
  if (primeTimer) {
    clearTimeout(primeTimer);
  }
});
</script>

<template>
  <div class="cli-manager">
    <div class="cli-overview">
      <div>
        <span>当前测活</span>
        <strong>{{ currentCliLabel }}</strong>
      </div>
      <div>
        <span>可用 CLI</span>
        <strong>{{ readyCliCount }} / {{ CLI_KINDS.length }}</strong>
      </div>
      <div
        v-for="kind in CLI_KINDS"
        :key="`overview-${kind}`"
        class="cli-overview-status"
        :class="statusTone(kind)"
      >
        <span class="cli-overview-label">
          <BrandIcon :brand="cliBrand(kind)" :size="14" />
          <span>{{ cliMeta[kind].label }}</span>
        </span>
        <strong>{{ statusLabel(kind) }}</strong>
      </div>
    </div>

    <div
      v-for="kind in CLI_KINDS"
      :key="kind"
      class="cli-card"
      :class="{ 'cli-card-current': settings.livenessCliKind === kind }"
    >
      <div class="cli-card-head">
        <div class="cli-card-title-group">
          <BrandIcon :brand="cliBrand(kind)" :size="22" />
          <div class="cli-card-title-copy">
            <span class="cli-card-title">{{ cliMeta[kind].label }}</span>
            <span class="cli-card-subtitle">{{ candidateSummary(kind) }}</span>
          </div>
        </div>
        <a-tag v-if="settings.livenessCliKind === kind" color="arcoblue" size="small">当前测活</a-tag>
        <a-button v-else size="mini" @click="setCurrentKind(kind)">设为测活</a-button>
        <span class="cli-card-status" :class="`cli-status-${state[kind].status}`">
          <template v-if="state[kind].status === 'checking'">校验中…</template>
          <template v-else-if="state[kind].status === 'ok'">✓ {{ state[kind].version || "可用" }}</template>
          <template v-else-if="state[kind].status === 'error'">✗ 未就绪</template>
          <template v-else>—</template>
        </span>
      </div>

      <div class="cli-card-input">
        <a-input
          :model-value="pathFor(kind)"
          placeholder="留空自动查找，或填写可执行文件路径"
          allow-clear
          @change="(value: string) => onPathChange(kind, value)"
        />
        <a-button @click="browse(kind)">浏览…</a-button>
        <a-button :loading="candidates[kind].loading" @click="rescan(kind)">重新扫描</a-button>
      </div>

      <div class="cli-card-foot">
        <code v-if="state[kind].status === 'ok' && state[kind].resolvedPath" class="cli-foot-ok">
          {{ state[kind].resolvedPath }}
        </code>
        <span v-else-if="state[kind].status === 'error'" class="cli-foot-error">
          {{ state[kind].message }}　可尝试安装：<code>{{ cliMeta[kind].install }}</code>
        </span>
        <span v-else-if="defaultCandidate(kind)" class="cli-foot-ok">
          命令行默认：<code>{{ defaultCandidate(kind)?.path }}</code>
        </span>
      </div>

      <div class="cli-card-actions">
        <a-button type="text" size="mini" @click="toggleCandidates(kind)">
          {{ candidates[kind].expanded ? "收起候选" : "显示所有候选" }}
        </a-button>
        <span v-if="candidates[kind].lastScannedAt" class="cli-scan-time">
          上次扫描 {{ candidates[kind].lastScannedAt }}
        </span>
        <a-button
          v-if="state[kind].status === 'error'"
          type="text"
          size="mini"
          @click="openInstallDoc(kind)"
        >
          安装文档
        </a-button>
      </div>

      <div v-if="candidates[kind].expanded" class="cli-candidates">
        <a-spin v-if="candidates[kind].loading" dot />
        <a-alert
          v-else-if="candidates[kind].message"
          type="error"
          :content="candidates[kind].message"
          show-icon
        />
        <a-empty
          v-else-if="!candidates[kind].scanned"
          description="候选扫描尚未完成，可点击重新扫描"
        />
        <a-empty v-else-if="candidates[kind].items.length === 0" description="未发现任何候选" />
        <template v-else>
          <button
            v-for="(candidate, index) in candidates[kind].items"
            :key="`${candidate.path}-${index}`"
            type="button"
            class="cli-candidate"
            :class="{
              'cli-candidate-selected': isSelectedCandidate(kind, candidate),
              'cli-candidate-invalid': !candidate.valid,
            }"
            @click="useCandidate(kind, candidate)"
          >
            <a-tag size="small" :color="candidate.valid ? 'green' : 'gray'">{{ candidate.source }}</a-tag>
            <a-tag v-if="candidate.isPathDefault" size="small" color="arcoblue">默认</a-tag>
            <span class="cli-candidate-version">
              {{ candidate.valid ? candidate.version || "可用" : "无效" }}
            </span>
            <span class="cli-candidate-path">{{ candidate.path }}</span>
            <span class="cli-candidate-action">
              {{ isSelectedCandidate(kind, candidate) ? "已选" : "使用" }}
            </span>
          </button>
        </template>
      </div>
    </div>
  </div>
</template>

<style scoped>
.cli-manager {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
  gap: 12px;
  width: 100%;
}

.cli-overview {
  grid-column: 1 / -1;
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(120px, 1fr));
  gap: 8px;
}

.cli-overview > div {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
  padding: 8px 10px;
  border: 1px solid var(--color-border-2);
  border-radius: 8px;
  background: var(--color-fill-1);
}

.cli-overview span {
  font-size: 12px;
  color: var(--color-text-3);
}

.cli-overview-label {
  display: inline-flex;
  align-items: center;
  gap: 5px;
}

.cli-overview-label .brand-icon {
  width: 14px;
  height: 14px;
}

.cli-overview strong {
  overflow: hidden;
  color: var(--color-text-1);
  font-size: 13px;
  font-weight: 600;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.cli-overview-ok {
  border-color: rgba(var(--green-6), 0.35) !important;
}

.cli-overview-error {
  border-color: rgba(var(--red-6), 0.35) !important;
}

.cli-overview-checking {
  border-color: rgba(var(--arcoblue-6), 0.35) !important;
}

.cli-card {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 12px;
  border: 1px solid var(--color-border-2);
  border-radius: 8px;
  background: var(--color-fill-1);
}

.cli-card-current {
  border-color: rgb(var(--arcoblue-6));
}

.cli-card-head {
  display: flex;
  align-items: center;
  gap: 8px;
}

.cli-card-title-group {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.cli-card-title-group .brand-icon {
  width: 22px;
  height: 22px;
}

.cli-card-title-copy {
  display: flex;
  min-width: 0;
  flex-direction: column;
}

.cli-card-title {
  font-weight: 600;
  color: var(--color-text-1);
}

.cli-card-subtitle {
  font-size: 12px;
  color: var(--color-text-3);
}

.cli-card-status {
  margin-left: auto;
  font-size: 12px;
  white-space: nowrap;
}

.cli-status-ok {
  color: rgb(var(--green-6));
}

.cli-status-error {
  color: rgb(var(--red-6));
}

.cli-status-checking {
  color: var(--color-text-3);
}

.cli-card-input {
  display: flex;
  gap: 8px;
}

.cli-card-foot {
  min-height: 18px;
  font-size: 12px;
  word-break: break-all;
}

.cli-foot-ok {
  color: var(--color-text-3);
}

.cli-foot-error {
  color: rgb(var(--red-6));
}

.cli-card-actions {
  display: flex;
  align-items: center;
  gap: 4px;
}

.cli-scan-time {
  font-size: 12px;
  color: var(--color-text-3);
}

.cli-candidates {
  display: flex;
  flex-direction: column;
  gap: 4px;
  max-height: 180px;
  overflow-y: auto;
  padding-top: 4px;
  border-top: 1px dashed var(--color-border-2);
}

.cli-candidate {
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 32px;
  padding: 6px 8px;
  text-align: left;
  background: transparent;
  border: 1px solid transparent;
  border-radius: 6px;
  cursor: pointer;
  font-size: 12px;
}

.cli-candidate:hover {
  background: var(--color-fill-2);
}

.cli-candidate-selected {
  border-color: rgb(var(--arcoblue-5));
  background: rgba(var(--arcoblue-1), 0.55);
}

.cli-candidate-invalid {
  cursor: not-allowed;
  opacity: 0.72;
}

.cli-candidate-version {
  flex-shrink: 0;
  color: var(--color-text-2);
}

.cli-candidate-path {
  flex: 1;
  color: var(--color-text-3);
  word-break: break-all;
}

.cli-candidate-action {
  flex-shrink: 0;
  color: rgb(var(--arcoblue-6));
}

@media (max-width: 560px) {
  .cli-card-input {
    flex-wrap: wrap;
  }

  .cli-card-input .arco-input-wrapper {
    flex-basis: 100%;
  }
}
</style>
