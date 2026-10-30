<script setup lang="ts">
import { computed } from "vue";
import BrandIcon from "../BrandIcon.vue";
import type {
  ProviderAppImportOption,
  ProviderAppImportTarget,
} from "../../stores/provider-types";
import magpieLogo from "../../assets/logos/magpie.svg";
import ccSwitchLogo from "../../assets/logos/cc-switch.png";

const props = defineProps<{ options: ProviderAppImportOption[] }>();
const emit = defineEmits<{
  select: [target: ProviderAppImportTarget];
}>();

const magpie = computed(() =>
  props.options.find((option) => option.target.app === "magpie"),
);
/// 只有 CC Switch 目标带 Agent，类型收窄后才能读 `target.agent`。
type AgentOption = ProviderAppImportOption & {
  target: Extract<ProviderAppImportTarget, { app: "ccSwitch" }>;
};
const ccSwitchAgents = computed(() =>
  props.options.filter(
    (option): option is AgentOption => option.target.app === "ccSwitch",
  ),
);
/// 卡片没有填完整 Key 时每个 CC Switch 目标都会给出同一条原因，逐行重复七遍
/// 只会把菜单撑高。原因说明一次，各行只保留 Agent 名和禁用态。
const sharedReason = computed(
  () =>
    ccSwitchAgents.value.find((option) => option.unavailableReason)
      ?.unavailableReason ?? "",
);
</script>

<template>
  <div class="provider-app-import">
    <!-- 两个目标应用各占一行应用抬头，具体导入项挂在各自抬头下面。只列 Agent
         名会让「导出到 CC Switch」看起来像是七个互不相干的应用。 -->
    <button
      v-if="magpie"
      type="button"
      role="menuitem"
      tabindex="-1"
      :disabled="Boolean(magpie.unavailableReason)"
      :title="magpie.unavailableReason || magpie.note || undefined"
      @click="emit('select', magpie.target)"
    >
      <i
        class="provider-app-import-magpie-icon"
        :style="{ '--magpie-logo': 'url(' + magpieLogo + ')' }"
        aria-hidden="true"
      />
      <strong>{{ magpie.label }}</strong>
      <small v-if="magpie.unavailableReason || magpie.note">{{
        magpie.unavailableReason || magpie.note
      }}</small>
    </button>

    <div v-if="ccSwitchAgents.length" class="provider-app-import-group">
      <div class="provider-app-import-app">
        <img :src="ccSwitchLogo" alt="" aria-hidden="true" />
        <strong>CC Switch</strong>
        <small>{{ ccSwitchAgents.length }} 个应用</small>
      </div>
      <p v-if="sharedReason" class="provider-app-import-reason">
        {{ sharedReason }}
      </p>
      <div class="provider-app-import-agents">
        <button
          v-for="option in ccSwitchAgents"
          :key="option.label"
          type="button"
          role="menuitem"
          tabindex="-1"
          :disabled="Boolean(option.unavailableReason)"
          :title="option.unavailableReason || undefined"
          @click="emit('select', option.target)"
        >
          <BrandIcon :brand="option.target.agent" :size="14" />
          <strong>{{ option.label }}</strong>
          <small v-if="option.unavailableReason">不可用</small>
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.provider-app-import {
  display: grid;
  min-width: 0;
  gap: 4px;
}

/* 一行一个导入目标：图标 | 名称 | 状态。每一项都是独立动作，排成一张
   紧凑清单比两列网格更好点，也不会让 Agent 名挤成残句。 */
.provider-app-import-group,
.provider-app-import-agents {
  display: grid;
  min-width: 0;
  gap: 1px;
}

.provider-app-import-app {
  display: grid;
  min-height: 28px;
  grid-template-columns: 16px minmax(0, 1fr) auto;
  align-items: center;
  gap: 8px;
  color: var(--surface-ink);
  padding: 2px 8px;
}

.provider-app-import-app > img,
.provider-app-import-magpie-icon {
  width: 15px;
  height: 15px;
  object-fit: contain;
}

.provider-app-import-app > strong {
  min-width: 0;
  overflow: hidden;
  font-size: 12px;
  font-weight: 650;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.provider-app-import-app > small {
  color: var(--surface-muted);
  font-size: 10px;
  white-space: nowrap;
}

/* 目标项退到应用图标右侧一档，读起来是「这个应用里的目标」，而不是和应用
   平级的另一个应用。 */
.provider-app-import-agents {
  padding-left: 8px;
}

.provider-app-import-reason {
  margin: 0;
  padding: 0 8px 2px;
  color: var(--surface-muted);
  font-size: 10px;
  line-height: 1.4;
}

.provider-app-import button {
  display: grid;
  width: 100%;
  min-height: 30px;
  grid-template-columns: 16px minmax(0, 1fr) auto;
  align-items: center;
  gap: 8px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--surface-ink-soft);
  cursor: pointer;
  padding: 4px 8px;
  text-align: left;
}

.provider-app-import button:hover:not(:disabled),
.provider-app-import button:focus-visible:not(:disabled) {
  background: rgba(22, 93, 255, 0.07);
  outline: none;
}

.provider-app-import button:disabled {
  cursor: not-allowed;
  opacity: 0.46;
}

.provider-app-import button > svg,
.provider-app-import button > img,
.provider-app-import button > .provider-app-import-magpie-icon {
  width: 15px;
  height: 15px;
  object-fit: contain;
}

.provider-app-import button > strong {
  min-width: 0;
  overflow: hidden;
  color: var(--surface-ink);
  font-size: 12px;
  font-weight: 550;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.provider-app-import button:disabled > strong {
  color: var(--surface-muted);
}

.provider-app-import button > small {
  overflow: hidden;
  color: var(--surface-muted);
  font-size: 10px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.provider-app-import-magpie-icon {
  background: currentColor;
  mask: var(--magpie-logo) center / contain no-repeat;
}
</style>
