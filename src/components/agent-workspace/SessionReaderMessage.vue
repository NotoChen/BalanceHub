<script setup lang="ts">
import { computed, ref, watch } from "vue";
import {
  Check,
  ChevronDown,
  Copy,
  FileDiff,
  Image,
  Terminal,
  Wrench,
  Brain,
  User,
  CircleAlert,
} from "@lucide/vue";
import { Message } from "@arco-design/web-vue";
import type {
  AgentCliKind,
  CliSessionMessage,
} from "../../stores/provider-types";
import type { SessionMessageLoadState } from "../../stores/session-reader-types";
import { sessionTime } from "../../utils/agent-session-display";
import { copyText } from "../../composables/useClipboard";
import AgentCliIcon from "../AgentCliIcon.vue";
import ContentDiff from "../ContentDiff.vue";
import CliSessionMessageContent from "../CliSessionMessageContent.vue";

const props = defineProps<{
  message: CliSessionMessage;
  agentKind: AgentCliKind;
  query: string;
  matched: boolean;
  loadState?: SessionMessageLoadState;
}>();
const emit = defineEmits<{
  expand: [id: string, fromStart: boolean];
}>();
const open = ref(props.matched);
const copied = ref(false);
const metadata = computed(() => props.message.metadata);
const foldable = computed(
  () =>
    props.message.role === "tool" ||
    metadata.value.kind !== "text" ||
    metadata.value.injected,
);
const kindLabel = computed(
  () =>
    ({
      text: metadata.value.injected
        ? "上下文"
        : props.message.role === "user"
          ? "你"
          : "回复",
      thinking: "思考过程",
      toolCall: "调用",
      toolResult: "结果",
      image: "图片",
      event: "会话事件",
    })[metadata.value.kind],
);
const label = computed(() =>
  metadata.value.toolName
    ? `${metadata.value.toolName} · ${kindLabel.value}`
    : kindLabel.value,
);
const statusLabel = computed(() =>
  metadata.value.status
    ? {
        succeeded: "成功",
        failed: "失败",
        running: "执行中",
        cancelled: "已取消",
      }[metadata.value.status]
    : "",
);
const full = computed(
  () => !props.message.hasMoreContent && props.message.contentOffset === 0,
);
const inlineImage = computed(() =>
  full.value &&
  /^data:image\/(?:png|jpe?g|gif|webp);base64,[\s\S]+$/i.test(
    props.message.content,
  )
    ? props.message.content
    : null,
);
const bodyVisible = computed(() => !foldable.value || open.value);
watch(
  () => props.matched,
  (matched) => {
    if (matched) open.value = true;
  },
);
watch(
  () => props.message.content,
  () => {
    copied.value = false;
  },
);
watch(
  bodyVisible,
  (visible) => {
    if (
      visible &&
      metadata.value.kind === "image" &&
      !full.value &&
      !props.loadState?.loading &&
      !props.loadState?.error
    )
      emit("expand", props.message.id, props.message.contentOffset > 0);
  },
  { immediate: true, flush: "post" },
);
function toggle() {
  open.value = !open.value;
}
async function copy() {
  try {
    await copyText(props.message.content);
    copied.value = true;
    Message.success(full.value ? "已复制消息内容" : "已复制当前载入的片段");
  } catch (error) {
    Message.error(error instanceof Error ? error.message : String(error));
  }
}
</script>

<template>
  <article
    class="session-reader-message"
    :class="[
      {
        'is-match': matched,
        'is-foldable': foldable,
        'is-open': bodyVisible,
        'is-failed': metadata.status === 'failed',
      },
      `is-${message.role}`,
    ]"
    :data-message-id="message.id"
  >
    <header class="session-reader-message-header">
      <button
        v-if="foldable"
        type="button"
        class="session-reader-fold"
        :aria-expanded="bodyVisible"
        @click="toggle"
      >
        <Brain v-if="metadata.kind === 'thinking'" :size="15" /><Image
          v-else-if="metadata.kind === 'image'"
          :size="15"
        /><FileDiff
          v-else-if="
            metadata.toolKind === 'edit' || metadata.toolKind === 'write'
          "
          :size="15"
        /><Terminal
          v-else-if="metadata.toolKind === 'shell'"
          :size="15"
        /><Wrench v-else :size="15" /> <strong>{{ label }}</strong
        ><span v-if="metadata.title" class="session-reader-tool-title">{{
          metadata.title
        }}</span
        ><ChevronDown class="session-reader-fold-chevron" :size="14" />
      </button>
      <template v-else
        ><User v-if="message.role === 'user'" :size="16" /><AgentCliIcon
          v-else
          :kind="agentKind"
          :size="18"
        /><strong>{{ kindLabel }}</strong></template
      >
      <span
        v-if="statusLabel"
        class="session-reader-tool-status"
        :class="`is-${metadata.status}`"
        >{{ statusLabel
        }}<template v-if="metadata.exitCode !== null">
          · 退出码 {{ metadata.exitCode }}</template
        ></span
      >
      <time v-if="message.timestamp" :title="sessionTime(message.timestamp)">{{
        sessionTime(message.timestamp, true)
      }}</time>
      <button
        v-if="bodyVisible && metadata.kind !== 'image'"
        type="button"
        class="session-reader-copy"
        :aria-label="
          copied ? '已复制内容' : full ? '复制消息内容' : '复制当前片段'
        "
        :title="copied ? '已复制' : full ? '复制内容' : '复制当前片段'"
        @click="copy"
      >
        <Check v-if="copied" :size="13" /><Copy v-else :size="13" />
      </button>
    </header>
    <div v-if="bodyVisible" class="session-reader-message-body">
      <p
        v-if="message.contentOffset > 0 && metadata.kind !== 'image'"
        class="session-reader-fragment-note"
      >
        已定位到匹配片段<button
          type="button"
          :disabled="loadState?.loading"
          @click="emit('expand', message.id, true)"
        >
          从这条消息的开头阅读
        </button>
      </p>
      <template v-if="metadata.kind === 'image'">
        <img
          v-if="inlineImage"
          class="session-reader-image"
          :src="inlineImage"
          alt="会话中的图片附件"
        />
        <p v-else-if="full" class="session-reader-fragment-note">
          原始记录没有包含可直接预览的图片。
        </p>
      </template>
      <template v-else-if="message.changes.length && full">
        <section
          v-for="(change, index) in message.changes"
          :key="`${change.path}:${index}`"
          class="session-reader-file-change"
        >
          <header>
            <FileDiff :size="14" /><strong>{{ change.path }}</strong>
          </header>
          <ContentDiff
            v-if="change.beforeKnown || change.patch"
            :original-text="change.before"
            :modified-text="change.after"
            :unified-patch="change.patch"
          />
          <template v-else
            ><p class="session-reader-fragment-note">
              此记录只包含写入内容，未记录修改前的文件。
            </p>
            <CliSessionMessageContent
              :content="change.after"
              :query="query"
              plain
          /></template>
        </section>
      </template>
      <CliSessionMessageContent
        v-else
        :content="message.content"
        :query="query"
        :plain="['toolCall', 'toolResult'].includes(metadata.kind)"
      />
      <p v-if="loadState?.error" class="agent-workspace-error" role="alert">
        <CircleAlert :size="13" />{{ loadState.error }}
      </p>
      <button
        v-if="message.hasMoreContent || (metadata.kind === 'image' && !full)"
        type="button"
        class="session-reader-expand"
        :disabled="loadState?.loading"
        @click="
          emit(
            'expand',
            message.id,
            metadata.kind === 'image' && message.contentOffset > 0,
          )
        "
      >
        {{
          loadState?.loading
            ? "正在读取…"
            : metadata.kind === "image"
              ? "重新加载图片"
              : "继续展开内容"
        }}<span>{{ message.contentLength.toLocaleString() }} 字符</span>
      </button>
    </div>
  </article>
</template>
