<script setup lang="ts">
import type { AgentNativeSetting } from "../stores/agent-settings-types";
import { computed, ref, toRef, watch } from "vue";
import { IconRefresh, IconSave, IconUndo } from "@arco-design/web-vue/es/icon";
import type {
  AgentCliKind,
  TemporaryCliSessionMode,
} from "../stores/provider-types";
import {
  type TemporaryCliRunSettings,
  type TemporaryCliSettingsCapabilities,
} from "../stores/temporary-cli-settings-types";
import { useTemporaryCliSettings } from "../composables/useTemporaryCliSettings";
import AgentNativeSettingField from "./AgentNativeSettingField.vue";
import TemporaryCliAssetPanel from "./TemporaryCliAssetPanel.vue";
import AgentSettingReference from "./AgentSettingReference.vue";
import { settingReadFailed } from "../utils/agent-setting-display";
import "../styles/modules/temporary-cli-settings.css";

const props = defineProps<{
  active: boolean;
  cliKind: AgentCliKind;
  cliPath: string;
  workdir: string;
  selectedModel: string;
  inheritedModel?: string;
  providerModels?: readonly string[];
  sessionMode: TemporaryCliSessionMode;
  canNameSession: boolean;
  sessionNamingHint: string;
  disabled?: boolean;
}>();
const model = defineModel<TemporaryCliRunSettings>({ required: true });
const sessionName = defineModel<string>("sessionName", { required: true });
const emit = defineEmits<{
  capabilities: [value: TemporaryCliSettingsCapabilities | null];
}>();
const request = computed(() => ({
  cliKind: props.cliKind,
  cliPath: props.cliPath,
  workdir: props.workdir,
  model: props.selectedModel,
  inheritedModel: props.inheritedModel ?? "",
  sessionMode: props.sessionMode,
  nativeOptions: model.value.nativeOptions,
}));
const ui = useTemporaryCliSettings(toRef(props, "active"), request, model);
const caps = ui.capabilities;
const hasReadError = computed(
  () =>
    caps.value?.model.state === "readError" ||
    Boolean(caps.value?.nativeSettings.some(settingReadFailed)),
);
const capabilityControlsDisabled = computed(
  () => props.disabled || !caps.value,
);
const activeTab = ref("model");
const saveExpanded = ref(false);
const assetQuery = ref("");
const assetCategory = ref("");
const unavailableOptions = computed(() =>
  caps.value
    ? Object.keys(model.value.nativeOptions).filter(
        (id) => !caps.value!.nativeSettings.some((field) => field.id === id),
      )
    : [],
);
const modelFields = computed(() => fieldsFor("model"));
const permissionFields = computed(() => fieldsFor("permissions"));
const toolFields = computed(() => fieldsFor("tools"));
const directories = computed({
  get: () => model.value.additionalDirectories.join("\n"),
  set: (value: string) =>
    patch("additionalDirectories", [
      ...new Set(
        value
          .split("\n")
          .map((path) => path.trim())
          .filter(Boolean),
      ),
    ]),
});
const keepSession = computed({
  get: () => props.sessionMode === "history" || model.value.keepSession,
  set: (value: boolean) => patch("keepSession", value),
});
watch(caps, (value) => emit("capabilities", value), { immediate: true });
watch(
  () => props.active && activeTab.value === "assets",
  (expanded) => {
    ui.assetsExpanded.value = expanded;
  },
  { immediate: true },
);
watch(
  () => props.active,
  (active) => {
    if (!active) {
      saveExpanded.value = false;
      assetQuery.value = "";
      assetCategory.value = "";
    }
  },
);
watch(
  () => [props.cliKind, props.cliPath, props.workdir],
  () => {
    assetQuery.value = "";
    assetCategory.value = "";
  },
);

function fieldsFor(section: AgentNativeSetting["section"]) {
  return (caps.value?.nativeSettings ?? []).filter(
    (field) =>
      field.section === section &&
      (model.value.nativeOptions[field.id] !== undefined ||
        !field.whenOption ||
        (model.value.nativeOptions[field.whenOption[0]] ??
          caps.value?.nativeSettings.find(
            (item) => item.id === field.whenOption![0],
          )?.currentValue) === field.whenOption[1]),
  );
}
function patch<K extends keyof TemporaryCliRunSettings>(
  key: K,
  value: TemporaryCliRunSettings[K],
) {
  model.value = { ...model.value, [key]: value };
}
function setNative(id: string, value: string | undefined) {
  const next = { ...model.value.nativeOptions };
  if (value === undefined) delete next[id];
  else next[id] = value;
  for (const field of caps.value?.nativeSettings ?? []) {
    if (field.whenOption?.[0] === id && field.whenOption[1] !== value)
      delete next[field.id];
  }
  patch("nativeOptions", next);
}
function clearUnavailable() {
  const next = { ...model.value.nativeOptions };
  for (const id of unavailableOptions.value) delete next[id];
  patch("nativeOptions", next);
}
function conflictFor(field: AgentNativeSetting) {
  if (
    !field.exclusiveGroup ||
    model.value.nativeOptions[field.id] === undefined
  )
    return "";
  const other = caps.value?.nativeSettings.find(
    (item) =>
      item.id !== field.id &&
      item.exclusiveGroup === field.exclusiveGroup &&
      model.value.nativeOptions[item.id] !== undefined,
  );
  return other
    ? "与「" + other.label + "」只能覆盖一项，请取消其中一项覆盖。"
    : "";
}
</script>

<template>
  <section class="cli-run-settings" aria-label="本次会话配置">
    <header class="cli-run-toolbar">
      <a-tabs
        v-model:active-key="activeTab"
        class="cli-run-tabs"
        :animation="false"
      >
        <a-tab-pane key="model" title="模型与上下文" />
        <a-tab-pane key="permissions" title="权限与工具" />
        <a-tab-pane key="assets"
          ><template #title
            >资产<span
              v-if="model.disabledAssets.length"
              class="cli-run-tab-count"
              >{{ model.disabledAssets.length }}</span
            ></template
          ></a-tab-pane
        >
        <a-tab-pane key="session" title="会话" />
      </a-tabs>
      <div class="cli-run-profile-actions">
        <span
          class="cli-run-refresh-state"
          role="status"
          :aria-label="ui.loading.value && caps ? '正在更新配置' : undefined"
          :title="ui.loading.value && caps ? '正在更新配置' : undefined"
        >
          <icon-refresh v-if="ui.loading.value && caps" spin />
        </span>
        <a-select
          v-if="ui.matchingProfiles.value.length"
          :model-value="ui.profileId.value || undefined"
          placeholder="使用方案"
          size="small"
          allow-clear
          :disabled="disabled"
          aria-label="使用启动方案"
          @change="ui.selectProfile(String($event || ''))"
        >
          <a-option
            v-for="profile in ui.matchingProfiles.value"
            :key="profile.id"
            :value="profile.id"
            >{{ profile.name }}</a-option
          >
        </a-select>
        <a-tooltip v-if="ui.modified.value" content="取消全部本次设置"
          ><a-button
            type="text"
            size="small"
            :disabled="disabled"
            aria-label="取消全部本次设置"
            @click="ui.selectProfile('')"
            ><template #icon><icon-undo /></template></a-button
        ></a-tooltip>
        <a-button
          type="text"
          size="small"
          :class="{ 'cli-run-profile-modified': ui.profileModified.value }"
          :title="
            ui.profileModified.value
              ? '当前修改尚未保存到方案'
              : '保存本次会话配置'
          "
          :disabled="disabled"
          :aria-expanded="saveExpanded"
          @click="saveExpanded = !saveExpanded"
          ><template #icon><icon-save /></template
          >{{ ui.profileModified.value ? "保存修改" : "保存方案" }}</a-button
        >
      </div>
    </header>
    <div v-if="saveExpanded" class="cli-run-profile-editor">
      <a-input
        v-model="ui.profileName.value"
        placeholder="方案名称，例如：代码审查"
        :max-length="60"
        size="small"
        :disabled="ui.saving.value"
        @press-enter="ui.saveProfile()"
      />
      <a-button
        size="small"
        type="primary"
        :loading="ui.saving.value"
        :disabled="!ui.canSaveProfile.value"
        @click="ui.saveProfile()"
        >{{ ui.profileSaveLabel.value }}</a-button
      >
      <a-button
        v-if="ui.profileId.value"
        size="small"
        :disabled="ui.saving.value || !ui.profileName.value.trim()"
        @click="ui.saveProfile(true)"
        >另存</a-button
      >
      <a-popconfirm
        v-if="ui.profileId.value"
        content="删除此启动方案？本次设置仍会保留。"
        @ok="ui.deleteProfile"
        ><a-button
          type="text"
          status="danger"
          size="small"
          :disabled="ui.saving.value"
          >删除</a-button
        ></a-popconfirm
      >
    </div>
    <p
      v-if="ui.profileError.value"
      class="cli-run-error cli-run-inline-error"
      role="alert"
    >
      {{ ui.profileError.value }}
    </p>

    <div class="cli-run-scroll" :aria-busy="ui.loading.value">
      <div v-if="ui.error.value" class="cli-run-error" role="alert">
        {{ ui.error.value
        }}<a-button type="text" size="small" @click="ui.loadCapabilities()"
          ><template #icon><icon-refresh /></template>重试</a-button
        >
      </div>
      <div
        v-if="ui.loading.value && !caps"
        class="cli-run-skeleton"
        role="status"
        aria-label="正在读取 Agent 配置"
      >
        <i /><i /><i /><span>正在读取所选 Agent 的配置…</span>
      </div>
      <p v-else-if="!caps && !ui.error.value" class="cli-run-empty">
        {{
          cliPath ? "选择运行环境后可调整配置" : "选择可用 Agent 后可调整配置"
        }}
      </p>
      <template v-if="caps">
        <div
          v-if="unavailableOptions.length"
          class="cli-run-error"
          role="alert"
        >
          当前 Agent 未提供方案中的
          {{ unavailableOptions.length }} 项选项。<a-button
            type="text"
            size="mini"
            :disabled="capabilityControlsDisabled"
            @click="clearUnavailable"
            >移除不可用选项</a-button
          >
        </div>
        <div
          v-if="activeTab === 'model'"
          class="cli-run-tab-panel"
          role="tabpanel"
          aria-label="模型与上下文"
        >
          <div class="cli-native-grid">
            <AgentNativeSettingField
              v-for="field in modelFields"
              :key="field.id"
              :field="field"
              :model-catalog="caps?.modelCatalog"
              :provider-models="providerModels"
              :value="model.nativeOptions[field.id]"
              :disabled="capabilityControlsDisabled"
              :conflict="conflictFor(field)"
              @update:value="setNative(field.id, $event)"
            />
          </div>
          <p v-if="!modelFields.length" class="cli-run-empty">
            当前 CLI 未公开可覆盖的模型参数。
          </p>
        </div>
        <div
          v-else-if="activeTab === 'permissions'"
          class="cli-run-tab-panel"
          role="tabpanel"
          aria-label="权限与工具"
        >
          <div class="cli-run-section-title">
            <strong>操作权限</strong>
          </div>
          <div class="cli-native-grid">
            <AgentNativeSettingField
              v-for="field in permissionFields"
              :key="field.id"
              :field="field"
              :model-catalog="caps?.modelCatalog"
              :provider-models="providerModels"
              :value="model.nativeOptions[field.id]"
              :disabled="capabilityControlsDisabled"
              @update:value="setNative(field.id, $event)"
            />
          </div>
          <div class="cli-run-section-title">
            <strong>工具范围</strong>
          </div>
          <div class="cli-native-grid">
            <AgentNativeSettingField
              v-for="field in toolFields"
              :key="field.id"
              :field="field"
              :model-catalog="caps?.modelCatalog"
              :provider-models="providerModels"
              :value="model.nativeOptions[field.id]"
              :disabled="capabilityControlsDisabled"
              @update:value="setNative(field.id, $event)"
            />
          </div>
        </div>
        <TemporaryCliAssetPanel
          v-else-if="activeTab === 'assets'"
          :key="cliKind + ':' + workdir"
          v-model="model"
          v-model:query="assetQuery"
          v-model:category="assetCategory"
          :caps="caps"
          :assets="ui.assets.value"
          :loaded="ui.assetsLoaded.value"
          :loading="ui.assetLoading.value"
          :error="ui.assetError.value"
          :notice="ui.assetNotice.value"
          :disabled="capabilityControlsDisabled"
          @refresh="ui.loadAssets(true)"
        />
        <div
          v-else
          class="cli-run-tab-panel cli-session-settings"
          role="tabpanel"
          aria-label="会话"
        >
          <section class="cli-session-retention">
            <div>
              <div class="cli-session-field-heading">
                <strong>保存会话记录</strong>
                <AgentSettingReference
                  :reference="caps.runReferences.keepSession"
                />
              </div>
              <p v-if="!keepSession">
                退出后清除本次会话记录，项目文件修改仍会保留。
              </p>
            </div>
            <a-switch
              v-model="keepSession"
              :disabled="
                capabilityControlsDisabled ||
                sessionMode === 'history' ||
                !caps.canDiscardSession
              "
              aria-label="保存会话记录"
            />
          </section>
          <p
            v-if="!caps.canDiscardSession && sessionMode === 'new'"
            class="cli-run-note"
          >
            {{ caps.retentionNote }}
          </p>
          <label v-if="canNameSession" class="cli-session-field"
            ><span>会话名称</span
            ><a-input
              v-model="sessionName"
              allow-clear
              :disabled="disabled"
              placeholder="例如：支付模块重构"
          /></label>
          <p v-else-if="sessionNamingHint" class="cli-run-note">
            {{ sessionNamingHint }}
          </p>
          <div class="cli-session-field">
            <div class="cli-session-field-heading">
              <label for="cli-initial-prompt">启动任务</label>
              <AgentSettingReference
                :reference="caps.runReferences.initialPrompt"
              />
            </div>
            <a-textarea
              id="cli-initial-prompt"
              :model-value="model.initialPrompt"
              :disabled="disabled"
              :auto-size="{ minRows: 3, maxRows: 7 }"
              placeholder="启动后直接交给 Agent 的第一条消息"
              @update:model-value="patch('initialPrompt', $event)"
            />
          </div>
          <div class="cli-session-rule-heading">
            <div class="cli-session-field-heading">
              <strong>工作规则</strong>
              <AgentSettingReference
                :reference="caps.runReferences.loadRules"
              />
            </div>
            <a-radio-group
              v-if="caps.canSkipRules"
              :model-value="model.loadRules ? 'inherit' : 'skip'"
              size="small"
              type="button"
              :disabled="capabilityControlsDisabled"
              @change="patch('loadRules', $event === 'inherit')"
              ><a-radio value="inherit">加载已有规则</a-radio
              ><a-radio value="skip">本次跳过</a-radio></a-radio-group
            ><span v-else>加载 Agent 已有规则</span>
          </div>
          <div class="cli-session-field">
            <div class="cli-session-field-heading">
              <label for="cli-session-instructions">本次补充要求</label>
              <AgentSettingReference
                :reference="caps.runReferences.instructions"
              />
            </div>
            <a-textarea
              id="cli-session-instructions"
              :model-value="model.instructions"
              :disabled="disabled"
              :auto-size="{ minRows: 3, maxRows: 7 }"
              placeholder="例如：先给出实施方案，得到确认后再修改文件"
              @update:model-value="patch('instructions', $event)"
            />
          </div>
          <div v-if="caps.canAddDirectories" class="cli-session-field">
            <div class="cli-session-field-heading">
              <label for="cli-session-directories">额外可访问目录</label>
              <AgentSettingReference
                :reference="caps.runReferences.additionalDirectories"
              />
            </div>
            <a-textarea
              id="cli-session-directories"
              v-model="directories"
              :disabled="capabilityControlsDisabled"
              :auto-size="{ minRows: 2, maxRows: 5 }"
              placeholder="每行一个绝对路径"
            />
          </div>
        </div>
        <details
          v-if="caps.notices.length"
          class="cli-run-notices"
          :class="{ 'has-read-error': hasReadError }"
          :open="hasReadError"
        >
          <summary>
            配置读取详情 <span>{{ caps.notices.length }}</span>
          </summary>
          <p v-for="notice in caps.notices" :key="notice">{{ notice }}</p>
        </details>
      </template>
    </div>
  </section>
</template>
