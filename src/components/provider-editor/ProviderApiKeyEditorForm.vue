<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { IconRefresh } from "@arco-design/web-vue/es/icon";
import type {
  ProviderApiKeyEditorContext,
  ProviderApiKeyExpiration,
  ProviderApiKeyPatch,
  ProviderApiKeySettings,
} from "../../stores/provider-types";
import { useLatestRequest } from "../../composables/useLatestRequest";
import {
  apiKeyRestrictionLines,
  apiKeySettingsPatch,
} from "../../utils/provider-api-key-settings";

const props = defineProps<{
  editing: boolean;
  loadContext: () => Promise<ProviderApiKeyEditorContext>;
  submit: (
    patch: ProviderApiKeyPatch,
    credentialRevision: number,
  ) => Promise<unknown>;
  isActive: () => boolean;
}>();
const emit = defineEmits<{ cancel: []; saved: [result: unknown] }>();
const context = ref<ProviderApiKeyEditorContext | null>(null);
const draft = ref<ProviderApiKeySettings | null>(null);
const customKey = ref("");
const allowIps = ref("");
const denyIps = ref("");
const loading = useLatestRequest({
  timeoutMs: 65_000,
  timeoutMessage: "读取 Key 设置超时，请重试",
});
const saving = useLatestRequest({
  timeoutMs: 65_000,
  timeoutMessage: "Key 保存请求超时，请先同步站点 Key 确认，避免重复操作",
});
const localError = ref("");
const error = computed(
  () => localError.value || loading.error.value || saving.error.value,
);
const patch = computed(() =>
  context.value && draft.value
    ? apiKeySettingsPatch(
        context.value,
        {
          ...draft.value,
          allowIps: apiKeyRestrictionLines(allowIps.value),
          denyIps: apiKeyRestrictionLines(denyIps.value),
        },
        !props.editing,
        customKey.value,
      )
    : {},
);
const hasChanges = computed(() => Object.keys(patch.value).length > 0);
const automatic = computed(() =>
  Boolean(
    context.value?.automaticGroup &&
    draft.value?.group === context.value.automaticGroup,
  ),
);
const groupOptions = computed(() => {
  if (!context.value) return [];
  const options = context.value.groups.map((group) => ({
    value: group.value,
    label: `${group.label}${group.rate == null ? "" : ` · ${group.rate}×`}${group.description ? ` · ${group.description}` : ""}`,
    disabled: false,
  }));
  if (context.value.groupClearable)
    options.unshift({
      value: "",
      label: context.value.defaultGroupLabel,
      disabled: false,
    });
  const current = context.value.settings.group;
  if (current && !options.some((option) => option.value === current)) {
    options.push({
      value: current,
      label: `${current}（当前分组，已不在可选范围）`,
      disabled: true,
    });
  }
  return options;
});
const neverExpires = computed({
  get: () => draft.value?.expiration.mode === "never",
  set: (never: boolean) => {
    if (!draft.value) return;
    draft.value.expiration = never
      ? { mode: "never" }
      : context.value?.expirationInDays
        ? { mode: "afterDays", days: 30 }
        : { mode: "at", timestamp: Date.now() + 30 * 86_400_000 };
  },
});
const expiryDays = computed({
  get: () =>
    draft.value?.expiration.mode === "afterDays"
      ? draft.value.expiration.days
      : 30,
  set: (days: number) => setExpiration({ mode: "afterDays", days }),
});
const expiryDate = computed({
  get: () =>
    draft.value?.expiration.mode === "at"
      ? draft.value.expiration.timestamp
      : undefined,
  set: (timestamp: number | undefined) =>
    setExpiration({ mode: "at", timestamp: timestamp ?? 0 }),
});
function setExpiration(expiration: ProviderApiKeyExpiration) {
  if (draft.value) draft.value.expiration = expiration;
}
const restrictionsPresent = computed(() =>
  Boolean(
    context.value &&
    (context.value.settings.allowIps.length ||
      context.value.settings.denyIps.length ||
      context.value.settings.modelLimitsEnabled ||
      context.value.settings.crossGroupRetry ||
      context.value.settings.autoGroups.length),
  ),
);
const spendingPresent = computed(
  () =>
    context.value &&
    Object.values(context.value.settings.spendingLimits).some(
      (limit) => limit > 0,
    ),
);

async function load() {
  if (!props.isActive()) return;
  await loading.run(props.loadContext, (result) => {
    if (!props.isActive()) return;
    context.value = result;
    draft.value = structuredClone(result.settings);
    allowIps.value = result.settings.allowIps.join("\n");
    denyIps.value = result.settings.denyIps.join("\n");
  });
}
async function save() {
  if (
    !props.isActive() ||
    !context.value ||
    !draft.value ||
    saving.loading.value
  )
    return;
  localError.value = "";
  if (!draft.value.name.trim()) {
    localError.value = "请填写站点 Key 名称";
    return;
  }
  const outgoing: ProviderApiKeyPatch = JSON.parse(JSON.stringify(patch.value));
  const revision = context.value.credentialRevision;
  await saving.run(
    () => props.submit(outgoing, revision),
    (result) => {
      if (props.isActive()) emit("saved", result);
    },
  );
}
onMounted(() => {
  void load();
});
</script>

<template>
  <form class="api-key-settings-form" @submit.prevent="save">
    <p class="api-key-settings-intro">
      {{
        editing
          ? "保存会立即更新站点上的这把 Key，其密钥值保持不变。"
          : "设置将直接提交到当前站点。"
      }}
    </p>
    <a-alert v-if="error" type="error" show-icon>{{ error }}</a-alert>
    <div
      v-if="!context || !draft"
      class="api-key-settings-loading"
      role="status"
    >
      <a-spin v-if="loading.loading.value" />
      <span>{{
        loading.loading.value
          ? "正在读取可选分组和 Key 设置…"
          : "Key 设置未能加载"
      }}</span>
      <a-button v-if="!loading.loading.value" @click="load"
        ><template #icon><IconRefresh /></template>重试</a-button
      >
    </div>
    <template v-else>
      <fieldset
        :disabled="saving.loading.value"
        class="api-key-settings-fields"
      >
        <div class="api-key-settings-grid">
          <label class="api-key-settings-field"
            ><span>站点 Key 名称</span
            ><a-input
              v-model="draft.name"
              allow-clear
              placeholder="例如：Claude Code、备用密钥"
              aria-label="站点 Key 名称"
          /></label>
          <label class="api-key-settings-field"
            ><span>分组</span
            ><a-select
              v-model="draft.group"
              :options="groupOptions"
              allow-search
              placeholder="选择可用分组"
              aria-label="Key 分组"
          /></label>
        </div>
        <div class="api-key-settings-grid">
          <section class="api-key-settings-box">
            <label class="api-key-settings-switch"
              ><span
                >{{ context.quotaLabel }}
                <small>{{ context.quotaUnit }}</small></span
              ><span
                >无限额度
                <a-switch
                  v-model="draft.quota.unlimited"
                  size="small"
                  aria-label="无限额度" /></span
            ></label>
            <a-input-number
              v-if="!draft.quota.unlimited"
              v-model="draft.quota.amount"
              :min="context.quotaMinimum"
              :precision="6"
              :placeholder="context.quotaLabel"
              :aria-label="context.quotaLabel"
            />
            <span v-else class="api-key-settings-hint"
              >不单独限制这把 Key 的累计用量</span
            >
          </section>
          <section class="api-key-settings-box">
            <label class="api-key-settings-switch"
              ><span>有效期</span
              ><span
                >永不过期
                <a-switch
                  v-model="neverExpires"
                  size="small"
                  aria-label="永不过期" /></span
            ></label>
            <a-input-number
              v-if="!neverExpires && context.expirationInDays"
              v-model="expiryDays"
              :min="1"
              :max="36500"
              :precision="0"
              aria-label="有效天数"
              ><template #suffix>天</template></a-input-number
            >
            <a-date-picker
              v-else-if="!neverExpires"
              v-model="expiryDate"
              show-time
              value-format="timestamp"
              format="YYYY-MM-DD HH:mm"
              :allow-clear="false"
              aria-label="到期时间"
            />
            <span v-else class="api-key-settings-hint"
              >持续有效，直到手动停用或删除</span
            >
          </section>
        </div>
        <label
          v-if="editing"
          class="api-key-settings-switch api-key-settings-status"
          ><span>启用 Key</span
          ><a-switch v-model="draft.enabled" aria-label="启用 Key"
        /></label>
        <details class="api-key-settings-details" :open="restrictionsPresent">
          <summary>访问限制</summary>
          <div class="api-key-settings-details-body">
            <div
              class="api-key-settings-grid"
              :class="{
                'api-key-settings-single': !context.supportsIpBlacklist,
              }"
            >
              <label class="api-key-settings-field"
                ><span>IP 白名单</span
                ><a-textarea
                  v-model="allowIps"
                  :auto-size="{ minRows: 3, maxRows: 6 }"
                  placeholder="每行一个 IP 或 CIDR 网段，留空不限制"
                  aria-label="IP 白名单"
              /></label>
              <label
                v-if="context.supportsIpBlacklist"
                class="api-key-settings-field"
                ><span>IP 黑名单</span
                ><a-textarea
                  v-model="denyIps"
                  :auto-size="{ minRows: 3, maxRows: 6 }"
                  placeholder="每行一个 IP 或 CIDR 网段，留空不拦截"
                  aria-label="IP 黑名单"
              /></label>
            </div>
            <section
              v-if="context.supportsModelLimits"
              class="api-key-settings-box"
            >
              <label class="api-key-settings-switch"
                ><span>只允许指定模型</span
                ><a-switch
                  v-model="draft.modelLimitsEnabled"
                  size="small"
                  aria-label="启用模型白名单"
              /></label>
              <template v-if="draft.modelLimitsEnabled">
                <a-select
                  v-model="draft.modelLimits"
                  multiple
                  allow-search
                  allow-create
                  :options="context.modelOptions"
                  placeholder="选择或输入允许调用的模型"
                  aria-label="模型白名单"
                />
                <span
                  v-if="context.modelOptionsError"
                  class="api-key-settings-hint"
                  >模型候选项读取失败，可直接输入模型名称：{{
                    context.modelOptionsError
                  }}</span
                >
                <span v-else class="api-key-settings-hint"
                  >候选项来自账号模型；实际调用还需要所选分组支持。</span
                >
              </template>
            </section>
            <section v-if="automatic" class="api-key-settings-box">
              <label
                v-if="context.supportsCrossGroupRetry"
                class="api-key-settings-switch"
                ><span>跨分组重试</span
                ><a-switch
                  v-model="draft.crossGroupRetry"
                  size="small"
                  aria-label="跨分组重试"
              /></label>
              <label v-if="context.autoGroups" class="api-key-settings-field"
                ><span>自动分组范围</span
                ><a-select
                  v-model="draft.autoGroups"
                  multiple
                  :max-tag-count="4"
                  :limit="context.autoGroups.maxCount"
                  :options="context.autoGroups.groups"
                  placeholder="留空使用站点默认范围"
                  aria-label="自动分组范围"
              /></label>
            </section>
          </div>
        </details>
        <details
          v-if="context.supportsSpendingLimits"
          class="api-key-settings-details"
          :open="spendingPresent || false"
        >
          <summary>周期消费上限</summary>
          <div class="api-key-settings-details-body">
            <span class="api-key-settings-hint"
              >按 USD 计算；0 表示不限制该周期。</span
            >
            <div class="api-key-settings-grid api-key-settings-thirds">
              <label class="api-key-settings-field"
                ><span>5 小时</span
                ><a-input-number
                  v-model="draft.spendingLimits.fiveHours"
                  :min="0"
                  :precision="6"
                  aria-label="5 小时消费上限"
              /></label>
              <label class="api-key-settings-field"
                ><span>1 天</span
                ><a-input-number
                  v-model="draft.spendingLimits.oneDay"
                  :min="0"
                  :precision="6"
                  aria-label="1 天消费上限"
              /></label>
              <label class="api-key-settings-field"
                ><span>7 天</span
                ><a-input-number
                  v-model="draft.spendingLimits.sevenDays"
                  :min="0"
                  :precision="6"
                  aria-label="7 天消费上限"
              /></label>
            </div>
          </div>
        </details>
        <details
          v-if="context.supportsCustomKey"
          class="api-key-settings-details"
        >
          <summary>自定义密钥值</summary>
          <label class="api-key-settings-field api-key-settings-details-body"
            ><a-input-password
              v-model="customKey"
              placeholder="留空由站点随机生成；自定义值至少 16 位"
              aria-label="自定义密钥值"
          /></label>
        </details>
      </fieldset>
    </template>
    <footer class="api-key-settings-footer">
      <a-button @click="emit('cancel')">取消</a-button>
      <a-button
        type="primary"
        html-type="submit"
        :loading="saving.loading.value"
        :disabled="!draft || !hasChanges || loading.loading.value"
        >{{ editing ? "保存设置" : "创建 Key" }}</a-button
      >
    </footer>
  </form>
</template>
