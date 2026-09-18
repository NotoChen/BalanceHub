<script setup lang="ts">
import { computed } from "vue";
import {
  useProviderStore,
  type AppSettings,
  type ProviderInput,
  type ProviderCheckInMethod,
  type ProviderTurnstileMode,
} from "../../stores/providers";
import { useProviderCheckInPolicy } from "../../composables/useProviderCheckInPolicy";
import type { SelectOption } from "./options";

const props = defineProps<{ draft: ProviderInput; settings: AppSettings }>();
const store = useProviderStore();
const { policy, loading, error } = useProviderCheckInPolicy({
  input: () => props.draft,
  active: () => true,
  preview: (input) => store.previewCheckInPolicy(input),
});

const checkInInheritsGlobal = computed({
  get: () => !props.draft.automation.checkInTime.trim(),
  set: (inherits: boolean) => {
    props.draft.automation.checkInTime = inherits ? "" : props.settings.checkInTime || "00:00";
  },
});

const methodOptions: SelectOption<ProviderCheckInMethod>[] = [
  { value: "auto", label: "自动选择（推荐）" },
  { value: "standard", label: "标准签到" },
  { value: "sessionSignIn", label: "会话签到（如 AnyRouter）" },
  { value: "freshLogin", label: "重新登录签到（如 AgentRouter）" },
];
const turnstileOptions: SelectOption<ProviderTurnstileMode>[] = [
  { value: "auto", label: "自动检测（推荐）" },
  { value: "always", label: "每次提交前验证" },
];
</script>

<template>
  <section class="provider-form-section">
    <h3 class="provider-form-section-title">签到与验证</h3>
    <div class="provider-form-section-body">
      <a-form-item class="provider-field" label="签到方式">
        <a-select v-model="draft.automation.checkInMethod" :options="methodOptions" :disabled="policy?.configurable === false" />
        <template #extra>
          <span role="status" aria-live="polite">
            <template v-if="loading">正在读取策略说明…</template>
            <template v-else-if="error">{{ error }}</template>
            <template v-else-if="policy">{{ policy.methodLabel }}：{{ policy.message }}</template>
          </span>
        </template>
      </a-form-item>
      <a-form-item class="provider-field" label="签到时间">
        <div class="provider-setting-controls">
          <a-checkbox v-model="checkInInheritsGlobal">跟随全局</a-checkbox>
          <a-time-picker
            v-if="!checkInInheritsGlobal"
            v-model="draft.automation.checkInTime"
            format="HH:mm"
            value-format="HH:mm"
            placeholder="00:00"
            disable-confirm
          />
        </div>
      </a-form-item>
      <a-form-item class="provider-field" label="站点防护">
        <a-checkbox v-model="draft.automation.autoShield">自动处理 WAF 与页面验证</a-checkbox>
        <template #extra>遇到站点防护时尝试自动处理，必要时打开验证小窗。</template>
      </a-form-item>
      <a-form-item class="provider-field" label="Turnstile 签到验证">
        <a-select v-model="draft.automation.turnstileMode" :options="turnstileOptions" :disabled="policy?.configurable === false" />
        <template #extra>自动检测会在签到要求验证时打开小窗，不受站点防护勾选项影响。</template>
      </a-form-item>
    </div>
  </section>
</template>
