<script setup lang="ts">
import { computed } from "vue";
import AgentCliIcon from "../AgentCliIcon.vue";
import { useCliRuntimeStore } from "../../stores/cli-runtime";
import type { AgentCliKind, Provider } from "../../stores/providers";
import { agentCliLabel } from "../../utils/cli-environment";
import {
  providerCardStatusLabel,
  providerCardStatusTitle,
  type ProviderCardTone,
} from "../../utils/provider-display";

/// 卡片标题右侧只有两件事：谁正在用这张卡，以及这张卡自己现在什么状态。
/// 四类卡片共用这一块，状态点和文案不再各写一套。
const props = withDefaults(
  defineProps<{
    provider: Provider;
    tone: ProviderCardTone;
    interactive?: boolean;
    activeCliCounts?: Partial<Record<AgentCliKind, number>>;
    warningLabel?: string;
  }>(),
  {
    interactive: true,
    activeCliCounts: () => ({}),
    warningLabel: "需关注",
  },
);

const emit = defineEmits<{
  openCliInstances: [provider: Provider, cliKind: AgentCliKind];
}>();

const store = useCliRuntimeStore();
const activeCliSignals = computed(() =>
  Object.entries(props.activeCliCounts)
    .filter((entry): entry is [AgentCliKind, number] => Number(entry[1]) > 0)
    .map(([cliKind, count]) => ({
      cliKind,
      count,
      label: agentCliLabel(store.cliEnvironmentProbe, cliKind),
    })),
);
const statusLabel = computed(() =>
  providerCardStatusLabel(props.tone, props.warningLabel),
);
/// 悬停说明只补卡片没写出来的异常消息，重复状态词属于噪音。
const statusTitle = computed(() => providerCardStatusTitle(props.provider));

function openCliInstances(cliKind: AgentCliKind) {
  emit("openCliInstances", props.provider, cliKind);
}
</script>

<template>
  <div class="provider-card-header-meta">
    <div
      v-if="activeCliSignals.length > 0"
      class="provider-card-cli-signals"
      aria-label="CLI 使用状态"
    >
      <template v-for="signal in activeCliSignals" :key="signal.cliKind">
        <button
          v-if="interactive"
          type="button"
          class="provider-card-cli-signal provider-card-cli-signal-active"
          :title="`查看 ${signal.count} 个 ${signal.label} 临时 CLI`"
          :aria-label="`查看 ${signal.count} 个 ${signal.label} 临时 CLI`"
          @click.stop="openCliInstances(signal.cliKind)"
          @pointerdown.stop
        >
          <AgentCliIcon :kind="signal.cliKind" :size="17" />
          <b>{{ signal.count }}</b>
        </button>
        <span
          v-else
          class="provider-card-cli-signal provider-card-cli-signal-active"
          :title="`${signal.count} 个 ${signal.label} 临时 CLI`"
          :aria-label="`${signal.count} 个 ${signal.label} 临时 CLI`"
        >
          <AgentCliIcon :kind="signal.cliKind" :size="17" />
          <b>{{ signal.count }}</b>
        </span>
      </template>
    </div>
    <span class="provider-card-status" :title="statusTitle">
      <i aria-hidden="true"></i>
      <span>{{ statusLabel }}</span>
    </span>
  </div>
</template>
