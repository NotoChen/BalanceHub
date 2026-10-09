<script setup lang="ts">
import { computed, inject, useId } from "vue";
import { LOGIN_ACCOUNTS_CONTEXT } from "../../composables/useLoginAccounts";
import { PROVIDER_CREDENTIALS_CONTEXT } from "../../composables/useProviderCredentials";
import { IconCheckCircle, IconLock, IconUser } from "@arco-design/web-vue/es/icon";
import type {
  AuthMode,
  Provider,
  ProviderAuthFieldDescriptor,
  ProviderInput,
  ProviderProtocolDescriptor,
} from "../../stores/providers";
import type { ApiKeyManagerOperation } from "../../composables/useApiKeyManager";
import { providerProtocolDescriptor } from "../../utils/provider-protocol";
import ProviderAuthIcon from "../ProviderAuthIcon.vue";
import ProviderCredentialFields from "./ProviderCredentialFields.vue";
import ProviderApiKeyVault from "./ProviderApiKeyVault.vue";

const loginAccounts = inject(LOGIN_ACCOUNTS_CONTEXT);
const providerCredentials = inject(PROVIDER_CREDENTIALS_CONTEXT);
const authGroupName = useId();

const props = defineProps<{
  draft: ProviderInput;
  disabled?: boolean;
  startingBrowserLogin: boolean;
  providerProtocols: ProviderProtocolDescriptor[];
  apiKeyOptions: ProviderInput["auth"]["apiKeyOptions"];
  apiKeyRemoteManaged: boolean;
  apiKeyManagerProvider: Provider | null;
  apiKeyManagerOperation: ApiKeyManagerOperation | null;
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
  "update:api-key-add-visible": [visible: boolean];
  "update:api-key-add-remark": [remark: string];
  "update:api-key-add-value": [value: string];
  "update:api-key-remark-visible": [visible: boolean];
  "update:api-key-remark-value": [remark: string];
  "sync-remote-api-keys": [];
  "open-api-key-create-editor": [];
  "open-api-key-settings-editor": [option: ProviderInput["auth"]["apiKeyOptions"][number]];
  "open-api-key-add-panel": [];
  "open-api-key-remark-editor": [option: ProviderInput["auth"]["apiKeyOptions"][number]];
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
const currentAuthMode = computed(() =>
  visibleAuthModes.value.find((mode) => mode.mode === props.draft.auth.mode),
);

const showAuthModePicker = computed(() => visibleAuthModes.value.length > 1);
const managingSavedApiKeys = computed(() => Boolean(props.draft.id && props.apiKeyManagerProvider));
const sharedFields = computed(() => {
  const fields = new Map<string, ProviderAuthFieldDescriptor[]>();
  for (const mode of visibleAuthModes.value) {
    for (const field of mode.fields) {
      const occurrences = fields.get(field.field) ?? [];
      occurrences.push(field);
      fields.set(field.field, occurrences);
    }
  }
  return [...fields.values()]
    .filter((occurrences) => occurrences.length > 1)
    .map((occurrences) => occurrences.find((field) => !field.readonly) ?? occurrences[0]!);
});
const credentialSections = computed(() => {
  const sharedNames = new Set(sharedFields.value.map((field) => field.field));
  return visibleAuthModes.value
    .filter((mode) => !(managingSavedApiKeys.value && mode.mode === "apiKey"))
    .map((mode) => ({
      ...mode,
      fields: mode.fields.filter((field) => !sharedNames.has(field.field)),
    }));
});
const showCredentialPanel = computed(() =>
  credentialSections.value.length > 0 || currentProtocol.value?.browserLoginSupported,
);

function updateField(field: ProviderAuthFieldDescriptor, value: string) {
  if (field.readonly) return;
  const key = field.field as keyof ProviderInput["auth"];
  if (!(key in props.draft.auth)) return;
  if (props.draft.auth[key] === value) return;
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

function selectMode(mode: AuthMode) {
  if (props.disabled || !visibleAuthModes.value.some((candidate) => candidate.mode === mode)) {
    return;
  }
  if (mode === props.draft.auth.mode) {
    return;
  }

  // 选择认证方式不等于重新登录或删除凭据；实际输入变更单独处理。
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
</script>

<template>
  <div
    :role="showAuthModePicker ? 'radiogroup' : 'group'"
    aria-label="认证方式"
    :aria-disabled="disabled || undefined"
    class="provider-form-page provider-credentials-page"
  >
    <section v-if="showCredentialPanel" class="provider-form-block provider-credential-active-panel">
      <header class="provider-form-block-header">
        <span class="provider-form-block-icon"><IconLock /></span>
        <div><strong>认证凭据</strong></div>
        <div v-if="draft.id && providerCredentials" class="provider-credential-tools">
          <a-button type="text" size="small" @click="providerCredentials.open(draft.id)">凭据详情</a-button>
        </div>
      </header>
      <div class="provider-form-block-body provider-primary-credentials">
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
        <div class="provider-auth-sections">
          <section
            v-for="mode in credentialSections"
            :key="mode.mode"
            class="provider-auth-section"
            :aria-label="mode.label"
          >
            <header class="provider-auth-section-heading">
              <span class="provider-auth-mode-icon">
                <ProviderAuthIcon :mode="mode.mode" :size="18" :decorative="true" />
              </span>
              <strong>{{ mode.label }}</strong>
              <label
                v-if="showAuthModePicker"
                class="provider-auth-section-choice"
                :class="{ 'is-selected': draft.auth.mode === mode.mode, 'is-disabled': disabled }"
              >
                <input
                  type="radio"
                  :name="authGroupName"
                  :value="mode.mode"
                  :checked="draft.auth.mode === mode.mode"
                  :disabled="disabled"
                  :aria-label="`使用${mode.label}认证`"
                  @change="selectMode(mode.mode)"
                />
                <span>{{ draft.auth.mode === mode.mode ? '当前使用' : '用于连接' }}</span>
              </label>
            </header>
            <div class="provider-field-grid">
              <p v-if="draft.auth.newApiSession && mode.mode === 'session'" class="provider-credential-inline-note provider-field-wide">
                <IconCheckCircle /> 已登录 {{ draft.auth.loginUsername || draft.auth.apiUser }}，会话自动续期。JWT 和 Cookie 可在“凭据详情”中查看和管理。
              </p>
              <ProviderCredentialFields v-else
                :fields="mode.fields"
                :required-fields="currentAuthMode?.requiredFields ?? []"
                :draft="draft"
                @copy-api-key="emit('copy-api-key')"
                @update-field="updateField"
              />
              <p v-if="mode.note && !(draft.auth.newApiSession && mode.mode === 'session')" class="provider-credential-inline-note provider-field-wide">
                {{ mode.note }}
              </p>
            </div>
          </section>
        </div>
        <div v-if="sharedFields.length" class="provider-field-grid provider-credential-shared-fields">
          <ProviderCredentialFields
            :fields="sharedFields"
            :required-fields="currentAuthMode?.requiredFields ?? []"
            :draft="draft"
            @copy-api-key="emit('copy-api-key')"
            @update-field="updateField"
          />
        </div>
        <slot name="assistant" />
      </div>
    </section>

    <ProviderApiKeyVault
      v-if="managingSavedApiKeys && apiKeyManagerProvider"
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
      @update:add-visible="emit('update:api-key-add-visible', $event)"
      @update:add-remark="emit('update:api-key-add-remark', $event)"
      @update:add-value="emit('update:api-key-add-value', $event)"
      @update:remark-visible="emit('update:api-key-remark-visible', $event)"
      @update:remark-value="emit('update:api-key-remark-value', $event)"
      @sync="emit('sync-remote-api-keys')"
      @show-create="emit('open-api-key-create-editor')"
      @show-settings="emit('open-api-key-settings-editor', $event)"
      @show-add="emit('open-api-key-add-panel')"
      @show-remark="emit('open-api-key-remark-editor', $event)"
      @add-local="emit('add-local-api-key')"
      @save-remark="emit('save-managed-api-key-remark')"
      @set-default="emit('set-default-managed-api-key', $event)"
      @copy="emit('copy-managed-api-key', $event)"
      @delete="emit('delete-managed-api-key', $event)"
    >
      <template #header-actions>
        <a-button v-if="!showCredentialPanel && draft.id && providerCredentials" type="text" size="small" @click="providerCredentials.open(draft.id)">凭据详情</a-button>
        <label
          v-if="showAuthModePicker"
          class="provider-auth-section-choice"
          :class="{ 'is-selected': draft.auth.mode === 'apiKey', 'is-disabled': disabled }"
        >
          <input
            type="radio"
            :name="authGroupName"
            value="apiKey"
            :checked="draft.auth.mode === 'apiKey'"
            :disabled="disabled"
            aria-label="使用 API Key 认证"
            @change="selectMode('apiKey')"
          />
          <span>{{ draft.auth.mode === 'apiKey' ? '当前使用' : '用于连接' }}</span>
        </label>
      </template>
    </ProviderApiKeyVault>
  </div>
</template>
