<script setup lang="ts">
import type { CliSessionDetail } from "../stores/provider-types";
import type { AgentSessionRow } from "../stores/agent-session-types";
import type {
  SessionMessageLoadState,
  SessionReaderNavigation,
} from "../stores/session-reader-types";
import SessionReader from "./agent-workspace/SessionReader.vue";
defineProps<{
  visible: boolean;
  loading: boolean;
  error: string;
  detail: CliSessionDetail | null;
  sessionRow: AgentSessionRow | null;
  query: string;
  paused: boolean;
  messageStates: Record<string, SessionMessageLoadState>;
}>();
const emit = defineEmits<{
  "update:visible": [visible: boolean];
  parent: [sessionRef: string];
  navigate: [navigation: SessionReaderNavigation];
  expand: [id: string, fromStart: boolean];
  pause: [];
  retry: [];
}>();
</script>
<template>
  <a-modal
    :visible="visible"
    width="min(1360px, calc(100vw - 40px))"
    modal-class="surface-modal cli-session-detail-modal"
    title="会话记录"
    title-align="start"
    :footer="false"
    closable
    mask-closable
    esc-to-close
    unmount-on-close
    @update:visible="emit('update:visible', $event)"
  >
    <SessionReader
      :detail="detail"
      :session-row="sessionRow"
      :loading="loading"
      :error="error"
      :query="query"
      :paused="paused"
      :message-states="messageStates"
      @parent="emit('parent', $event)"
      @navigate="emit('navigate', $event)"
      @expand="(id, fromStart) => emit('expand', id, fromStart)"
      @pause="emit('pause')"
      @retry="emit('retry')"
    />
  </a-modal>
</template>
