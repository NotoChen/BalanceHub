<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { Copy, FileDown, Send } from "@lucide/vue";
import { Message } from "@arco-design/web-vue";
import type {
  AgentCliDescriptor,
  AgentCliKind,
  Provider,
  TemporaryCliLaunchContext,
} from "../../stores/provider-types";
import type { AgentHandoff } from "../../stores/agent-handoff-types";
import type { AgentSessionRow } from "../../stores/agent-session-types";
import { prepareAgentHandoff } from "../../api/agent-handoff";
import { cancelAgentSessionQuery } from "../../api/agent-sessions";
import { withTimeout } from "../../utils/promise-timeout";
import { copyText } from "../../composables/useClipboard";
import { fullAgentCliLabel } from "../../utils/cli-environment";
import { providerDisplayLabel } from "../../utils/provider-display";
import { sessionTime } from "../../utils/agent-session-display";
import AgentCliIcon from "../AgentCliIcon.vue";
import "../../styles/modules/agent-handoff.css";

const props = defineProps<{
  visible: boolean;
  row: AgentSessionRow | null;
  scopeRevision: string;
  providers: readonly Provider[];
  agents: readonly AgentCliDescriptor[];
}>();
const emit = defineEmits<{
  close: [];
  launch: [provider: Provider, cliKind: AgentCliKind, context: TemporaryCliLaunchContext];
}>();

const loading = ref(false);
const error = ref("");
const handoff = ref<AgentHandoff | null>(null);
const consumerId = `agent-handoff-${Math.random().toString(36).slice(2)}`;
let request = 0;
let disposed = false;

const launchableProviders = computed(() =>
  props.providers.filter((provider) => provider.actions.temporaryCli),
);
const selectedProviderId = ref("");
const selectedCliKind = ref<AgentCliKind>("codex");
const selectedProvider = computed(
  () =>
    launchableProviders.value.find(
      (provider) => provider.identity.id === selectedProviderId.value,
    ) ?? null,
);
const targetAgentOptions = computed(() =>
  props.agents.map((agent) => ({ value: agent.kind, label: agent.label })),
);
const initialPrompt = computed(() => {
  const current = handoff.value;
  if (!current) return "";
  return [
    `请先阅读交接文件：${current.path}`,
    "该文件记录了上一个会话的最近上下文与待办，阅读后继续完成任务。",
  ].join("\n");
});
const sourceTime = computed(() =>
  props.row?.session.updatedAt
    ? sessionTime(props.row.session.updatedAt)
    : "无更新时间",
);

function providerLabel(provider: Provider) {
  return providerDisplayLabel(provider) || "未命名中转站";
}

function release() {
  const id = request;
  request += 1;
  loading.value = false;
  if (id > 0) {
    void cancelAgentSessionQuery({ consumerId, requestId: id }).catch(() => {});
  }
}

async function load() {
  const row = props.row;
  if (!row) return;
  const id = ++request;
  loading.value = true;
  error.value = "";
  handoff.value = null;
  try {
    const result = await withTimeout(
      prepareAgentHandoff({
        consumerId,
        requestId: id,
        scopeRevision: props.scopeRevision,
        sessionRef: row.sessionRef,
      }),
      30_000,
      "生成会话交接超时",
    );
    if (disposed || id !== request) return;
    handoff.value = result;
    if (!selectedProviderId.value && launchableProviders.value.length) {
      selectedProviderId.value = launchableProviders.value[0].identity.id;
    }
    if (!props.agents.some((agent) => agent.kind === selectedCliKind.value)) {
      selectedCliKind.value =
        props.agents.find((agent) => agent.kind === result.cliKind)?.kind ??
        props.agents[0]?.kind ??
        result.cliKind;
    }
  } catch (cause) {
    if (disposed || id !== request) return;
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    if (!disposed && id === request) loading.value = false;
  }
}

async function copy(value: string, label: string) {
  try {
    await copyText(value);
    Message.success(label);
  } catch (cause) {
    Message.error(cause instanceof Error ? cause.message : String(cause));
  }
}

function launch() {
  const provider = selectedProvider.value;
  const current = handoff.value;
  if (!provider || !current) return;
  emit("launch", provider, selectedCliKind.value, {
    workdir: current.workdir,
    initialPrompt: initialPrompt.value,
  });
  close();
}

function close() {
  release();
  handoff.value = null;
  error.value = "";
  emit("close");
}

watch(
  () => props.visible,
  (visible) => {
    if (!visible) {
      release();
      return;
    }
    void load();
  },
  { immediate: true },
);
onBeforeUnmount(() => {
  disposed = true;
  release();
});
</script>

<template>
  <a-modal
    :visible="visible"
    width="min(880px, calc(100vw - 32px))"
    modal-class="surface-modal agent-handoff-modal"
    :footer="false"
    closable
    mask-closable
    esc-to-close
    unmount-on-close
    @cancel="close"
  >
    <template #title>
      <div class="agent-handoff-heading">
        <strong>交接上下文</strong>
        <span v-if="row" class="agent-handoff-title" :title="row.session.title">{{
          row.session.title
        }}</span>
      </div>
    </template>
    <div class="agent-handoff-body">
      <p class="agent-handoff-note">
        读取该原生会话最近的用户与助手消息，生成独立的交接文件；
        不修改原始会话，也不改写项目文件。
      </p>
      <p v-if="error" class="agent-handoff-error" role="alert">{{ error }}</p>
      <p v-if="loading" class="agent-handoff-state" role="status">
        <a-spin :size="14" /> 正在读取会话最近内容并生成交接文件…
      </p>
      <template v-else-if="handoff">
        <div class="agent-handoff-meta">
          <span><AgentCliIcon :kind="handoff.cliKind" :size="14" />{{ fullAgentCliLabel(handoff.cliKind) }}</span>
          <span>{{ handoff.messages }} 条消息</span>
          <span v-if="handoff.model">{{ handoff.model }}</span>
          <span>{{ sourceTime }}</span>
          <span v-if="handoff.truncated" class="agent-handoff-warning">只读取到部分内容，交接可能不完整</span>
        </div>
        <div class="agent-handoff-path">
          <span :title="handoff.path">{{ handoff.path }}</span>
          <a-button size="mini" @click="copy(handoff.path, '已复制交接文件路径')">
            <template #icon><Copy :size="12" /></template>路径
          </a-button>
          <a-button size="mini" @click="copy(handoff.content, '已复制交接全文')">
            <template #icon><FileDown :size="12" /></template>全文
          </a-button>
        </div>
        <pre class="agent-handoff-preview" tabindex="0" aria-label="交接内容预览">{{ handoff.content }}</pre>
        <section class="agent-handoff-target">
          <header>
            <h4>在目标 Agent 中继续</h4>
            <span>启动参数会带上交接文件路径；目标 Agent 读取后继续工作。</span>
          </header>
          <div v-if="!launchableProviders.length" class="agent-handoff-state">
            没有可临时启动的中转站账号，请先刷新中转站授权。交接全文仍可复制到任意 Agent。
          </div>
          <template v-else>
            <div class="agent-handoff-fields">
              <label>
                <span>中转站账号</span>
                <a-select v-model="selectedProviderId" placeholder="选择启动使用的中转站">
                  <a-option
                    v-for="provider in launchableProviders"
                    :key="provider.identity.id"
                    :value="provider.identity.id"
                    >{{ providerLabel(provider) }}</a-option
                  >
                </a-select>
              </label>
              <label>
                <span>目标 Agent</span>
                <a-select v-model="selectedCliKind" placeholder="选择目标 Agent">
                  <a-option
                    v-for="agent in targetAgentOptions"
                    :key="agent.value"
                    :value="agent.value"
                    >{{ agent.label }}</a-option
                  >
                </a-select>
              </label>
            </div>
            <p class="agent-handoff-prompt-note">
              启动后第一条消息：{{ initialPrompt }}
            </p>
          </template>
        </section>
        <footer class="agent-handoff-actions">
          <a-button @click="close">关闭</a-button>
          <a-button
            type="primary"
            :disabled="!selectedProvider"
            @click="launch"
          >
            <template #icon><Send :size="14" /></template>选择目录并启动
          </a-button>
        </footer>
      </template>
      <footer v-else-if="!loading" class="agent-handoff-actions">
        <a-button @click="close">关闭</a-button>
      </footer>
    </div>
  </a-modal>
</template>
