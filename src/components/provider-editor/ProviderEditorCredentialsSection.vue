<script setup lang="ts">
import { computed, inject } from "vue";
import { LOGIN_ACCOUNTS_CONTEXT } from "../../composables/useLoginAccounts";
import { PROVIDER_CREDENTIALS_CONTEXT } from "../../composables/useProviderCredentials";
import { IconCheckCircle, IconLock, IconRight, IconUser } from "@arco-design/web-vue/es/icon";
import type {
  AuthMode,
  Provider,
  ProviderAuthFieldDescriptor,
  ProviderInput,
  ProviderProtocolDescriptor,
} from "../../stores/providers";
import type { ApiKeyManagerOperation } from "../../composables/useApiKeyManager";
import { providerProtocolDescriptor } from "../../utils/provider-protocol";
import { credentialFieldHasValue, missingCredentialRequirements } from "../../composables/provider-credential-rules";
import ProviderAuthIcon from "../ProviderAuthIcon.vue";
import ProviderCredentialFields from "./ProviderCredentialFields.vue";
import ProviderApiKeyVault from "./ProviderApiKeyVault.vue";
import RadioChoiceGroup from "../RadioChoiceGroup.vue";

const loginAccounts = inject(LOGIN_ACCOUNTS_CONTEXT);
const providerCredentials = inject(PROVIDER_CREDENTIALS_CONTEXT);

const props = defineProps<{
  draft: ProviderInput;
  disabled?: boolean;
  startingBrowserLogin: boolean;
  providerProtocols: ProviderProtocolDescriptor[];
  apiKeyOptions: ProviderInput["auth"]["apiKeyOptions"];
  apiKeyRemoteManaged: boolean;
  apiKeyManagerProvider: Provider | null;
  apiKeyManagerOperation: ApiKeyManagerOperation | null;
  apiKeyCreateVisible: boolean;
  apiKeyCreateName: string;
  apiKeyAddVisible: boolean;
  apiKeyAddRemark: string;
  apiKeyAddValue: string;
  apiKeyRemarkVisible: boolean;
  apiKeyRemarkValue: string;
  apiKeyRemarkTarget: ProviderInput["auth"]["apiKeyOptions"][number] | null;
}>();

const emit = defineEmits<{
  "copy-api-key": [];
  "login-and-import": [];
  "update:api-key-create-visible": [visible: boolean];
  "update:api-key-create-name": [name: string];
  "update:api-key-add-visible": [visible: boolean];
  "update:api-key-add-remark": [remark: string];
  "update:api-key-add-value": [value: string];
  "update:api-key-remark-visible": [visible: boolean];
  "update:api-key-remark-value": [remark: string];
  "sync-remote-api-keys": [];
  "open-api-key-create-panel": [];
  "open-api-key-add-panel": [];
  "open-api-key-remark-editor": [option: ProviderInput["auth"]["apiKeyOptions"][number]];
  "create-managed-api-key": [];
  "add-local-api-key": [];
  "save-managed-api-key-remark": [];
  "set-default-managed-api-key": [option: ProviderInput["auth"]["apiKeyOptions"][number]];
  "copy-managed-api-key": [option: ProviderInput["auth"]["apiKeyOptions"][number]];
  "delete-managed-api-key": [option: ProviderInput["auth"]["apiKeyOptions"][number]];
}>();

const currentProtocol = computed(() =>
  providerProtocolDescriptor(props.providerProtocols, props.draft.identity.protocol),
);

const visibleAuthModes = computed(() => currentProtocol.value?.authModes ?? []);
const authChoices = computed(() => visibleAuthModes.value.map((mode) => ({
  value: mode.mode,
  label: mode.label,
  description: mode.description,
})));

const currentAuthMode = computed(() =>
  visibleAuthModes.value.find((mode) => mode.mode === props.draft.auth.mode),
);

const showAuthModePicker = computed(() => visibleAuthModes.value.length > 1);
const managingSavedApiKeys = computed(() => Boolean(props.draft.id && props.apiKeyManagerProvider));
const showActiveCredentialFields = computed(() =>
  !(managingSavedApiKeys.value && props.draft.auth.mode === "apiKey"),
);
const activeFields = computed(() => currentAuthMode.value?.fields ?? []);

const secondaryModes = computed(() => {
  const modes = visibleAuthModes.value;
  const index = modes.findIndex((mode) => mode.mode === props.draft.auth.mode);
  const secondary = index < 0 ? [] : modes.slice(index + 1);
  return managingSavedApiKeys.value
    ? secondary.filter((mode) => mode.mode !== "apiKey")
    : secondary;
});

const secondaryOrderText = computed(() => secondaryModes.value.map((mode) => mode.label).join(" → "));

function fieldsForMode(mode: AuthMode) {
  return visibleAuthModes.value.find((candidate) => candidate.mode === mode)?.fields ?? [];
}

function fieldValue(field: ProviderAuthFieldDescriptor) {
  const value = props.draft.auth[field.field as keyof ProviderInput["auth"]];
  return typeof value === "string" ? value : "";
}

function updateField(field: ProviderAuthFieldDescriptor, value: string) {
  if (field.readonly) return;
  const key = field.field as keyof ProviderInput["auth"];
  if (!(key in props.draft.auth)) return;
  (props.draft.auth as unknown as Record<string, unknown>)[key as string] = value;
  if (field.field === "apiKey") {
    syncApiKeySelection();
  } else if (field.field === "sessionCookie") {
    props.draft.auth.newApiSession = null;
  } else if (field.field === "accessToken") {
    invalidateRefreshTokenChain();
  } else if (field.field === "loginUsername" || field.field === "loginPassword") {
    invalidatePasswordSession();
  }
}

function stageHasValue(mode: AuthMode) {
  const schema = visibleAuthModes.value.find((candidate) => candidate.mode === mode);
  return fieldsForMode(mode).some((field) => Boolean(fieldValue(field).trim()))
    || Boolean(schema?.requiredAnyFields.some((field) => credentialFieldHasValue(props.draft, field)));
}

function stageStatus(mode: AuthMode) {
  const auth = props.draft.auth;
  if (mode === "password") {
    if (auth.loginUsername.trim() && auth.loginPassword.trim()) return "可切换";
    if (auth.loginUsername.trim()) return "账号已补全";
    return "可补全";
  }
  if (mode === "session") {
    const schema = visibleAuthModes.value.find((candidate) => candidate.mode === mode);
    if (schema && missingCredentialRequirements(props.draft, schema).length === 0) return "已保存";
    return props.draft.auth.mode === "password" ? "登录后生成" : "待补充";
  }
  if (mode === "accessToken") {
    if (auth.accessToken.trim()) return "已保存";
    return props.draft.auth.mode === "password" || props.draft.auth.mode === "session"
      ? "可获取"
      : "待补充";
  }
  if (auth.apiKey.trim()) return "已保存";
  return props.draft.auth.mode === "apiKey" ? "待补充" : "可获取";
}

function stageStatusClass(mode: AuthMode) {
  return stageHasValue(mode) ? "ready" : "pending";
}

function selectMode(mode: AuthMode) {
  if (!visibleAuthModes.value.some((candidate) => candidate.mode === mode)) {
    return;
  }
  if (mode === props.draft.auth.mode) {
    return;
  }

  // 切入账号密码时强制重新登录；从账号密码切到下游认证时保留已建立的会话，
  // 这样用户不需要再次粘贴 Cookie。
  if (mode === "password" && props.draft.auth.mode !== "password") {
    props.draft.auth.newApiSession = null;
    props.draft.auth.sessionCookie = "";
    props.draft.auth.apiUser = "";
    clearTokenChain();
  }
  props.draft.auth.mode = mode;
}

function invalidatePasswordSession() {
  if (props.draft.auth.mode === "password") {
    props.draft.auth.newApiSession = null;
    props.draft.auth.sessionCookie = "";
    props.draft.auth.apiUser = "";
    clearTokenChain();
  }
}

function clearTokenChain() {
  props.draft.auth.accessToken = "";
  props.draft.auth.refreshToken = "";
  props.draft.auth.accessTokenExpiresAt = null;
}

function invalidateRefreshTokenChain() {
  props.draft.auth.refreshToken = "";
  props.draft.auth.accessTokenExpiresAt = null;
}

function syncApiKeySelection() {
  const current = props.draft.auth.apiKey.trim();
  props.draft.auth.apiKeyTokenId =
    props.apiKeyOptions.find((option) => option.key.trim() === current)?.tokenId || "";
}

function activeLabel() {
  if (props.draft.auth.newApiSession && props.draft.auth.mode === "session") return "登录会话";
  return currentAuthMode.value?.label || "认证凭据";
}
</script>

<template>
  <div class="provider-form-page provider-credentials-page">
    <section class="provider-form-block provider-credential-active-panel">
      <header class="provider-form-block-header">
        <span class="provider-form-block-icon"><IconLock /></span>
        <div>
          <strong>{{ showAuthModePicker ? '连接认证' : activeLabel() }}</strong>
          <small>{{ currentAuthMode?.description }}</small>
        </div>
        <div v-if="draft.id && providerCredentials" class="provider-credential-tools">
          <a-button type="text" size="small" @click="providerCredentials.open(draft.id)">凭据详情</a-button>
        </div>
      </header>
      <div v-if="showAuthModePicker || showActiveCredentialFields || currentProtocol?.browserLoginSupported" class="provider-form-block-body provider-primary-credentials">
        <RadioChoiceGroup
          v-if="showAuthModePicker"
          :model-value="draft.auth.mode"
          :options="authChoices"
          :disabled="disabled"
          label="认证方式"
          class="provider-auth-mode-grid"
          option-class="provider-auth-mode-option"
          @update:model-value="selectMode"
        >
          <template #default="{ option, selected }">
            <span class="provider-auth-mode-icon">
              <ProviderAuthIcon :mode="option.value" :size="20" :decorative="true" />
            </span>
            <span class="provider-auth-mode-copy"><strong>{{ option.label }}</strong></span>
            <IconCheckCircle v-if="selected" class="provider-auth-mode-check" />
          </template>
        </RadioChoiceGroup>
        <div v-if="currentProtocol?.browserLoginSupported" class="provider-browser-login">
          <IconUser class="provider-browser-login-icon" />
          <div class="provider-browser-login-copy">
            <strong>在浏览器中登录</strong>
            <span>完成站点登录后，自动导入认证凭据。</span>
          </div>
          <div class="provider-browser-login-actions">
            <a-button v-if="loginAccounts" type="text" size="small" @click="loginAccounts.open(draft.auth.browserBinding?.accountId ?? undefined)">管理登录账号</a-button>
            <a-button
              :loading="startingBrowserLogin"
              :disabled="disabled || !draft.identity.baseUrl.trim()"
              @click="emit('login-and-import')"
            >登录并导入</a-button>
          </div>
        </div>
        <div v-if="showActiveCredentialFields" class="provider-field-grid">
          <p v-if="draft.auth.newApiSession && draft.auth.mode === 'session'" class="provider-credential-inline-note provider-field-wide">
            <IconCheckCircle /> 已登录 {{ draft.auth.loginUsername || draft.auth.apiUser }}，会话自动续期。JWT 和 Cookie 可在“凭据详情”中查看和管理。
          </p>
          <ProviderCredentialFields v-else
            :fields="activeFields"
            :required-fields="currentAuthMode?.requiredFields ?? []"
            :draft="draft"
            @copy-api-key="emit('copy-api-key')"
            @update-field="updateField"
          />
          <p v-if="currentAuthMode?.note && !(draft.auth.newApiSession && draft.auth.mode === 'session')" class="provider-credential-inline-note provider-field-wide">
            {{ currentAuthMode.note }}
          </p>
        </div>
        <slot name="assistant" />
      </div>
    </section>

    <section v-if="secondaryModes.length > 0" class="provider-form-block provider-credential-chain">
      <header class="provider-form-block-header provider-credential-chain-heading">
        <span class="provider-form-block-icon provider-form-block-icon-neutral"><IconLock /></span>
        <div><strong>补充凭据</strong><small>需要时展开查看或填写。</small></div>
        <span class="provider-credential-chain-order">{{ secondaryOrderText }}</span>
      </header>
      <div class="provider-credential-chain-list">
        <details
          v-for="mode in secondaryModes"
          :key="mode.mode"
          class="provider-credential-stage"
          :class="[`is-${mode.mode}`, { 'has-value': stageHasValue(mode.mode) }]"
          :open="mode.mode === 'apiKey' && apiKeyOptions.length > 0"
        >
          <summary>
            <span class="provider-credential-stage-main">
              <span class="provider-credential-stage-icon">
                <ProviderAuthIcon :mode="mode.mode" :size="16" :decorative="true" />
              </span>
              <strong>{{ mode.label }}</strong>
            </span>
            <span class="provider-credential-stage-status" :class="stageStatusClass(mode.mode)">
              {{ stageStatus(mode.mode) }}
            </span>
            <IconRight class="provider-credential-stage-chevron" />
          </summary>

          <div class="provider-credential-stage-fields provider-field-grid">
            <ProviderCredentialFields
              :fields="mode.fields"
              :required-fields="mode.requiredFields"
              :draft="draft"
              @copy-api-key="emit('copy-api-key')"
              @update-field="updateField"
            />
            <p v-if="mode.note" class="provider-credential-inline-note provider-field-wide">
              {{ mode.note }}
            </p>
          </div>
        </details>
      </div>
    </section>

    <ProviderApiKeyVault
      v-if="managingSavedApiKeys && apiKeyManagerProvider"
      :create-visible="apiKeyCreateVisible"
      :create-name="apiKeyCreateName"
      :add-visible="apiKeyAddVisible"
      :add-remark="apiKeyAddRemark"
      :add-value="apiKeyAddValue"
      :remark-visible="apiKeyRemarkVisible"
      :remark-value="apiKeyRemarkValue"
      :remark-target="apiKeyRemarkTarget"
      :provider="apiKeyManagerProvider"
      :operation="apiKeyManagerOperation"
      :keys="apiKeyOptions"
      :remote-managed="apiKeyRemoteManaged"
      @update:create-visible="emit('update:api-key-create-visible', $event)"
      @update:create-name="emit('update:api-key-create-name', $event)"
      @update:add-visible="emit('update:api-key-add-visible', $event)"
      @update:add-remark="emit('update:api-key-add-remark', $event)"
      @update:add-value="emit('update:api-key-add-value', $event)"
      @update:remark-visible="emit('update:api-key-remark-visible', $event)"
      @update:remark-value="emit('update:api-key-remark-value', $event)"
      @sync="emit('sync-remote-api-keys')"
      @show-create="emit('open-api-key-create-panel')"
      @show-add="emit('open-api-key-add-panel')"
      @show-remark="emit('open-api-key-remark-editor', $event)"
      @create="emit('create-managed-api-key')"
      @add-local="emit('add-local-api-key')"
      @save-remark="emit('save-managed-api-key-remark')"
      @set-default="emit('set-default-managed-api-key', $event)"
      @copy="emit('copy-managed-api-key', $event)"
      @delete="emit('delete-managed-api-key', $event)"
    />
  </div>
</template>
