<script setup lang="ts">
import { computed, onScopeDispose, ref, watch } from "vue";
import { save } from "@tauri-apps/plugin-dialog";
import { Download, Send } from "@lucide/vue";
import { exportOAuthAccount, sendOAuthToCpa } from "../../api/oauth-accounts";
import { useProviderStore } from "../../stores/providers";
import { confirmAction } from "../../composables/provider-credential-dialogs";
import type {
  OAuthAccountDetails,
  OAuthExportFormat,
} from "../../stores/oauth-types";
const props = defineProps<{ account: OAuthAccountDetails }>();
const emit = defineEmits<{ changed: [] }>();
const store = useProviderStore();
const format = ref<OAuthExportFormat>(
  props.account.exportOptions.find((option) => option.available)?.format ??
    "balanceHub",
);
const pause = ref(true);
const gatewayId = ref<string>();
const busy = ref(false);
const error = ref("");
const resultMessage = ref("");
let disposed = false;
onScopeDispose(() => {
  disposed = true;
});
const current = computed(() =>
  props.account.exportOptions.find((option) => option.format === format.value),
);
/// A credential can be usable in BalanceHub and unusable everywhere else (for
/// example when it was issued to another app's OAuth client). The panel then
/// leads with that one fact instead of repeating "不支持" under seven tiles.
const availableOptions = computed(() =>
  props.account.exportOptions.filter((option) => option.available),
);
const unavailableOptions = computed(() =>
  props.account.exportOptions.filter((option) => !option.available),
);
const onlyBalanceHub = computed(() => availableOptions.value.length <= 1);
const sharedReason = computed(() => {
  const reasons = [
    ...new Set(
      unavailableOptions.value
        .map((option) => option.reason?.trim())
        .filter((reason): reason is string => Boolean(reason)),
    ),
  ];
  return reasons.length === 1 ? reasons[0] : "";
});
watch(
  () => props.account.exportOptions,
  (options) => {
    if (options.some((option) => option.format === format.value && option.available)) {
      return;
    }
    format.value =
      options.find((option) => option.available)?.format ?? "balanceHub";
  },
);
const gateways = computed(() =>
  store.providers
    .filter((provider) => provider.actions.cpaGateway)
    .map((provider) => ({
      value: provider.identity.id,
      label: provider.displayLabel,
    })),
);
const cpaAvailable = computed(
  () =>
    props.account.exportOptions.find((option) => option.format === "cpa")
      ?.available,
);

async function exportFile() {
  if (busy.value || !current.value?.available) return;
  busy.value = true;
  error.value = "";
  resultMessage.value = "";
  const id = props.account.provider.identity.id;
  const selectedFormat = format.value;
  try {
    const path = await save({
      title: "导出账号授权",
      defaultPath: current.value.fileName,
      filters: [{ name: "JSON 授权文件", extensions: ["json"] }],
    });
    if (!path || disposed) return;
    const result = await exportOAuthAccount(
      id,
      selectedFormat,
      path,
      pause.value,
    );
    store.upsertProvider(result.provider);
    if (!disposed) {
      resultMessage.value =
        result.warning || `授权文件已保存。${result.instructions}`;
      emit("changed");
    }
  } catch (cause) {
    if (!disposed) error.value = String(cause);
  } finally {
    busy.value = false;
  }
}
async function transfer() {
  if (busy.value || !gatewayId.value || !cpaAvailable.value) return;
  const targetId = gatewayId.value;
  const id = props.account.provider.identity.id;
  const targetName =
    gateways.value.find((gateway) => gateway.value === targetId)?.label ||
    "所选 CPA";
  busy.value = true;
  try {
    const confirmed = await confirmAction(
      "发送授权到 CPA",
      `这会将原始账号授权上传到「${targetName}」，并暂停 BalanceHub 对该账号的续期，由 CPA 接管。`,
      "发送并交接续期",
      "warning",
    );
    if (!confirmed || disposed) return;
    error.value = "";
    resultMessage.value = "";
    const result = await sendOAuthToCpa(id, targetId);
    store.upsertProvider(result.provider);
    if (!disposed) {
      resultMessage.value = result.instructions;
      emit("changed");
    }
    void store.refreshByIds([targetId]).catch(() => {});
  } catch (cause) {
    if (!disposed) {
      error.value = String(cause);
      emit("changed");
    }
  } finally {
    busy.value = false;
  }
}
</script>
<template>
  <section class="oauth-export-panel" aria-label="导出授权">
    <h3>导出到</h3>
    <a-alert v-if="onlyBalanceHub" type="info">
      {{
        sharedReason ||
        "这份授权目前只能在 BalanceHub 内使用。其他目标需要重新登录同一个账号，才能接管它的续期。"
      }}
    </a-alert>
    <div
      class="oauth-export-targets"
      role="radiogroup"
      aria-label="授权文件格式"
    >
      <label
        v-for="option in availableOptions"
        :key="option.format"
        :class="{ selected: format === option.format }"
        :title="option.instructions"
        ><input
          v-model="format"
          type="radio"
          :value="option.format"
          :disabled="busy"
        /><span>{{ option.label }}</span></label
      >
    </div>
    <p v-if="current" class="oauth-export-hint">{{ current.instructions }}</p>
    <div
      v-if="unavailableOptions.length"
      class="oauth-export-unavailable"
      aria-label="不能直接交接的目标"
    >
      <h4>不能直接交给这些目标</h4>
      <ul>
        <li v-for="option in unavailableOptions" :key="option.format">
          <strong>{{ option.label }}</strong>
          <span>{{ option.reason }}</span>
        </li>
      </ul>
    </div>
    <div class="oauth-export-footer">
      <a-checkbox v-model="pause">导出后暂停本机续期</a-checkbox
      ><a-button
        type="primary"
        :disabled="!current?.available"
        :loading="busy"
        @click="exportFile"
        ><template #icon><Download :size="15" /></template
        >保存授权文件</a-button
      >
    </div>
    <div v-if="gateways.length && cpaAvailable" class="oauth-cpa-send">
      <a-select
        v-model="gatewayId"
        :options="gateways"
        placeholder="选择已连接的 CPA"
      /><a-button :disabled="!gatewayId" :loading="busy" @click="transfer"
        ><template #icon><Send :size="14" /></template>发送到 CPA</a-button
      >
    </div>
    <a-alert v-if="error" type="error">{{ error }}</a-alert
    ><a-alert v-if="resultMessage" type="info">{{ resultMessage }}</a-alert>
  </section>
</template>
<style scoped>
.oauth-export-panel {
  display: grid;
  gap: 13px;
}
h3 {
  margin: 0;
  font-size: 13px;
  font-weight: 550;
  color: var(--color-text-1);
}
.oauth-export-targets {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 7px;
}
.oauth-export-targets label {
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 4px;
  min-height: 46px;
  padding: 5px 8px;
  border: 1px solid var(--color-border-2);
  border-radius: 7px;
  color: var(--color-text-2);
  font-size: 12px;
  cursor: pointer;
  transition:
    border-color 0.15s,
    background 0.15s;
}
.oauth-export-targets input {
  position: absolute;
  opacity: 0;
  width: 1px;
  height: 1px;
}
.oauth-export-targets label:focus-within {
  outline: 2px solid rgb(var(--primary-4));
  outline-offset: 2px;
}
.oauth-export-targets label.selected {
  background: var(--color-primary-light-1);
  border-color: rgb(var(--primary-5));
  color: rgb(var(--primary-6));
}
.oauth-export-hint {
  margin: 0;
  font-size: 12px;
  line-height: 1.65;
  color: var(--color-text-3);
}
/* A target that cannot take this credential is a fact about the target, not a
   choice, so it reads as a short explanation instead of seven dead tiles. */
.oauth-export-unavailable {
  display: grid;
  gap: 5px;
  padding: 9px 11px;
  border: 1px solid var(--color-border-2);
  border-radius: 7px;
  background: var(--color-fill-1);
}
.oauth-export-unavailable h4 {
  margin: 0;
  color: var(--color-text-2);
  font-size: 11px;
  font-weight: 550;
}
.oauth-export-unavailable ul {
  display: grid;
  gap: 4px;
  margin: 0;
  padding: 0;
  list-style: none;
}
.oauth-export-unavailable li {
  display: grid;
  grid-template-columns: 86px minmax(0, 1fr);
  gap: 8px;
  font-size: 11px;
  line-height: 1.55;
  color: var(--color-text-3);
}
.oauth-export-unavailable strong {
  color: var(--color-text-2);
  font-weight: 500;
}
.oauth-export-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.oauth-cpa-send {
  display: flex;
  align-items: center;
  gap: 10px;
  padding-top: 13px;
  border-top: 1px solid var(--color-border-2);
}
.oauth-cpa-send :deep(.arco-select) {
  flex: 1;
}
@media (max-width: 620px) {
  .oauth-export-targets {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
  .oauth-export-footer {
    flex-wrap: wrap;
  }
}
</style>
