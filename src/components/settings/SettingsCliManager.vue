<script setup lang="ts">
import { onMounted, reactive } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Message } from "@arco-design/web-vue";
import {
  useProviderStore,
  type AppSettings,
  type CliCandidate,
  type LivenessCliKind,
} from "../../stores/providers";

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
}

const CLI_KINDS: LivenessCliKind[] = ["codex", "claudeCode"];

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
  codex: { items: [], loading: false, expanded: false },
  claudeCode: { items: [], loading: false, expanded: false },
});

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

async function validate(kind: LivenessCliKind) {
  const card = state[kind];
  card.status = "checking";
  card.message = "";
  try {
    const result = await store.checkCliPath(kind, pathFor(kind).trim());
    card.status = "ok";
    card.version = result.version;
    card.resolvedPath = result.path;
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
  const card = state[kind];
  card.status = "checking";
  card.message = "";
  try {
    // 传空 => 后端按内置规则自动发现。
    const result = await store.checkCliPath(kind, "");
    setPath(kind, result.path);
    card.status = "ok";
    card.version = result.version;
    card.resolvedPath = result.path;
    Message.success(`已找到 ${cliMeta[kind].label}：${result.version || result.path}`);
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
  } catch (error) {
    Message.error(error instanceof Error ? error.message : String(error));
  }
}

async function toggleCandidates(kind: LivenessCliKind) {
  const view = candidates[kind];
  view.expanded = !view.expanded;
  if (view.expanded && view.items.length === 0) {
    view.loading = true;
    try {
      // 传空 => 枚举系统上所有候选（含来源/版本/有效性）。
      view.items = await store.listCliCandidates(kind, "");
    } catch (error) {
      Message.error(error instanceof Error ? error.message : String(error));
    } finally {
      view.loading = false;
    }
  }
}

async function useCandidate(kind: LivenessCliKind, candidate: CliCandidate) {
  setPath(kind, candidate.path);
  candidates[kind].expanded = false;
  await validate(kind);
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
    void validate(kind);
  }
});
</script>

<template>
  <div class="cli-manager">
    <div
      v-for="kind in CLI_KINDS"
      :key="kind"
      class="cli-card"
      :class="{ 'cli-card-current': settings.livenessCliKind === kind }"
    >
      <div class="cli-card-head">
        <span class="cli-card-title">{{ cliMeta[kind].label }}</span>
        <a-tag v-if="settings.livenessCliKind === kind" color="arcoblue" size="small">当前测活</a-tag>
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
        <a-button @click="rescan(kind)">重新扫描</a-button>
      </div>

      <div class="cli-card-foot">
        <code v-if="state[kind].status === 'ok' && state[kind].resolvedPath" class="cli-foot-ok">
          {{ state[kind].resolvedPath }}
        </code>
        <span v-else-if="state[kind].status === 'error'" class="cli-foot-error">
          {{ state[kind].message }}　可尝试安装：<code>{{ cliMeta[kind].install }}</code>
        </span>
      </div>

      <div class="cli-card-actions">
        <a-button type="text" size="mini" @click="toggleCandidates(kind)">
          {{ candidates[kind].expanded ? "收起候选" : "显示所有候选" }}
        </a-button>
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
        <a-empty v-else-if="candidates[kind].items.length === 0" description="未发现任何候选" />
        <template v-else>
          <button
            v-for="(candidate, index) in candidates[kind].items"
            :key="`${candidate.path}-${index}`"
            type="button"
            class="cli-candidate"
            @click="useCandidate(kind, candidate)"
          >
            <a-tag size="small" :color="candidate.valid ? 'green' : 'gray'">{{ candidate.source }}</a-tag>
            <span class="cli-candidate-version">
              {{ candidate.valid ? candidate.version || "可用" : "无效" }}
            </span>
            <span class="cli-candidate-path">{{ candidate.path }}</span>
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

.cli-card-title {
  font-weight: 600;
  color: var(--color-text-1);
}

.cli-card-status {
  margin-left: auto;
  font-size: 12px;
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
  gap: 4px;
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
  padding: 4px 6px;
  text-align: left;
  background: transparent;
  border: none;
  border-radius: 6px;
  cursor: pointer;
  font-size: 12px;
}

.cli-candidate:hover {
  background: var(--color-fill-2);
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
</style>
