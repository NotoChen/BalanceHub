<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from "vue";
import { useVirtualizer } from "@tanstack/vue-virtual";
import { ArrowRight, FolderOpen, Terminal } from "@lucide/vue";
import { Message } from "@arco-design/web-vue";
import type {
  AgentCliDescriptor,
  AgentRuntimeSession,
} from "../../stores/provider-types";
import type {
  AgentSessionRow,
  AgentSessionWorkspace,
} from "../../stores/agent-session-types";
import type { SessionListPosition } from "../../stores/session-reader-types";
import { copyText } from "../../composables/useClipboard";
import {
  groupSessionRows,
  sessionModelLabel,
  sessionTime,
} from "../../utils/agent-session-display";
import AgentCliIcon from "../AgentCliIcon.vue";
import AgentSessionRelation from "./AgentSessionRelation.vue";
import SessionHighlight from "./SessionHighlight.vue";

const props = withDefaults(
  defineProps<{
    rows: AgentSessionRow[];
    agents?: readonly AgentCliDescriptor[];
    workspaces?: readonly AgentSessionWorkspace[];
    selectedSessionRef?: string;
    allowResume?: boolean;
    resumeBusy?: (sessionRef: string) => boolean;
    runtimeSessions?: AgentRuntimeSession[];
    activatingId?: string | null;
    disabled?: boolean;
    groupBy?: "time" | "project";
    positionKey?: string;
    position?: SessionListPosition | null;
  }>(),
  {
    agents: () => [],
    workspaces: () => [],
    selectedSessionRef: "",
    allowResume: false,
    runtimeSessions: () => [],
    activatingId: null,
    disabled: false,
    groupBy: "time",
    positionKey: "",
    position: null,
  },
);
const emit = defineEmits<{
  detail: [row: AgentSessionRow];
  resume: [row: AgentSessionRow];
  parent: [sessionRef: string];
  activate: [session: AgentRuntimeSession];
  position: [position: SessionListPosition];
}>();
const viewport = ref<HTMLElement | null>(null);
const initialPosition =
  props.position?.key === props.positionKey ? props.position : null;
const entries = computed(() => {
  const labels = new Map(
    props.agents.map((agent) => [agent.kind, agent.label]),
  );
  const terminals = new Map(
    props.runtimeSessions
      .filter((session) => session.actions.canActivateTerminal)
      .map((session) => [session.runtimeId, session]),
  );
  return groupSessionRows(props.rows, props.groupBy, props.workspaces).map(
    ({ row, heading }) => {
      return {
        row,
        heading,
        label: labels.get(row.session.cliKind) || row.session.cliKind,
        terminals: [...new Set(row.runtimeIds)].flatMap((id) => {
          const terminal = terminals.get(id);
          return terminal ? [terminal] : [];
        }),
      };
    },
  );
});
const virtual = useVirtualizer(
  computed(() => ({
    count: entries.value.length,
    getScrollElement: () => viewport.value,
    getItemKey: (index: number) => entries.value[index].row.sessionRef,
    estimateSize: () => 142,
    overscan: 5,
    initialOffset: initialPosition?.offset ?? 0,
    initialMeasurementsCache: initialPosition?.measurements ?? [],
  })),
);
const items = computed(() => virtual.value.getVirtualItems());
function measure(element: unknown) {
  virtual.value.measureElement(element instanceof HTMLElement ? element : null);
}
onBeforeUnmount(() => {
  emit("position", {
    key: props.positionKey,
    offset: virtual.value.scrollOffset ?? 0,
    measurements: virtual.value.takeSnapshot(),
  });
});
async function copy(value: string) {
  try {
    await copyText(value);
    Message.success("已复制会话目录");
  } catch (error) {
    Message.error(error instanceof Error ? error.message : String(error));
  }
}
</script>
<template>
  <div
    ref="viewport"
    class="agent-session-list"
    role="list"
    aria-label="历史会话列表"
  >
    <div
      :style="{ height: `${virtual.getTotalSize()}px`, position: 'relative' }"
    >
      <div
        v-for="item in items"
        :key="String(item.key)"
        :ref="measure"
        :data-index="item.index"
        class="agent-session-virtual-row"
        :style="{ transform: `translateY(${item.start}px)` }"
        role="listitem"
      >
        <h3
          v-if="entries[item.index].heading"
          class="agent-session-day-heading"
          :title="entries[item.index].heading"
        >
          {{ entries[item.index].heading }}
        </h3>
        <article
          class="agent-session-row"
          :class="{
            'is-selected':
              entries[item.index].row.sessionRef === selectedSessionRef,
          }"
          :data-session-ref="entries[item.index].row.sessionRef"
          :data-session-role="entries[item.index].row.role"
        >
          <button
            type="button"
            class="agent-session-select"
            :disabled="disabled"
            :aria-current="
              entries[item.index].row.sessionRef === selectedSessionRef
                ? 'true'
                : undefined
            "
            @click="emit('detail', entries[item.index].row)"
          >
            <AgentCliIcon
              :kind="entries[item.index].row.session.cliKind"
              :size="22"
            />
            <div class="agent-session-row-body">
              <div class="agent-session-title-line">
                <strong>{{ entries[item.index].row.session.title }}</strong
                ><time
                  :datetime="
                    entries[item.index].row.session.updatedAt || undefined
                  "
                  :title="
                    sessionTime(entries[item.index].row.session.updatedAt)
                  "
                  >{{
                    sessionTime(entries[item.index].row.session.updatedAt, true)
                  }}</time
                >
              </div>
              <p
                v-if="entries[item.index].row.searchHit"
                class="agent-session-preview is-hit"
              >
                <SessionHighlight
                  :text="entries[item.index].row.searchHit!.snippet"
                  :terms="entries[item.index].row.searchHit!.terms"
                />
              </p>
              <p
                v-else-if="entries[item.index].row.session.preview"
                class="agent-session-preview"
              >
                {{ entries[item.index].row.session.preview }}
              </p>
            </div>
          </button>
          <div class="agent-session-meta">
            <span>{{ entries[item.index].label }}</span
            ><span
              class="agent-session-model"
              :title="sessionModelLabel(entries[item.index].row.session)"
              >{{ sessionModelLabel(entries[item.index].row.session) }}</span
            ><AgentSessionRelation
              :row="entries[item.index].row"
              @parent="emit('parent', $event)"
            /><span
              v-if="entries[item.index].row.activityState === 'active'"
              class="agent-session-activity is-active"
              >活动中</span
            ><span
              v-if="entries[item.index].row.session.archived"
              class="agent-session-archived"
              >已归档</span
            >
          </div>
          <div class="agent-session-row-footer">
            <button
              type="button"
              class="agent-session-workdir"
              :title="`复制目录：${entries[item.index].row.session.workdir}`"
              @click="copy(entries[item.index].row.session.workdir)"
            >
              <FolderOpen :size="12" /><span>{{
                entries[item.index].row.session.workdir
              }}</span>
            </button>
            <div class="agent-session-row-actions">
              <button
                v-for="runtime in entries[item.index].terminals"
                :key="runtime.runtimeId"
                type="button"
                :disabled="disabled || activatingId === runtime.runtimeId"
                @click="emit('activate', runtime)"
              >
                <Terminal :size="12" />终端</button
              ><button
                v-if="
                  allowResume &&
                  entries[item.index].row.activityState !== 'active'
                "
                type="button"
                :disabled="
                  disabled ||
                  !entries[item.index].row.session.canResume ||
                  resumeBusy?.(entries[item.index].row.sessionRef)
                "
                :title="entries[item.index].row.resumeReason || '继续会话'"
                @click="emit('resume', entries[item.index].row)"
              >
                <ArrowRight :size="12" />继续
              </button>
            </div>
          </div>
        </article>
      </div>
    </div>
  </div>
</template>
