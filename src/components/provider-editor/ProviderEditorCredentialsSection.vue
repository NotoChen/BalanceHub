<script setup lang="ts">
import { computed } from "vue";
import { IconCheckCircle, IconLock, IconRight } from "@arco-design/web-vue/es/icon";
import type {
  AuthMode,
  ProviderAuthFieldDescriptor,
  ProviderInput,
  ProviderProtocolDescriptor,
} from "../../stores/providers";
import { providerProtocolDescriptor } from "../../utils/provider-protocol";
import ProviderAuthIcon from "../ProviderAuthIcon.vue";
import ProviderCredentialFields from "./ProviderCredentialFields.vue";
import ProviderApiKeyPicker from "./ProviderApiKeyPicker.vue";

const props = defineProps<{
  draft: ProviderInput;
  providerProtocols: ProviderProtocolDescriptor[];
  apiKeyOptions: ProviderInput["auth"]["apiKeyOptions"];
}>();

const emit = defineEmits<{
  "copy-api-key": [];
  "select-api-key": [option: ProviderInput["auth"]["apiKeyOptions"][number]];
}>();

const currentProtocol = computed(() =>
  providerProtocolDescriptor(props.providerProtocols, props.draft.identity.protocol),
);

const visibleAuthModes = computed(() => currentProtocol.value?.authModes ?? []);

const currentAuthMode = computed(() =>
  visibleAuthModes.value.find((mode) => mode.mode === props.draft.auth.mode),
);

const activeFields = computed(() => currentAuthMode.value?.fields ?? []);

const secondaryModes = computed(() => {
  const modes = visibleAuthModes.value;
  const index = modes.findIndex((mode) => mode.mode === props.draft.auth.mode);
  return index < 0 ? [] : modes.slice(index + 1);
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
  } else if (field.field === "accessToken") {
    invalidateRefreshTokenChain();
  } else if (field.field === "loginUsername" || field.field === "loginPassword") {
    invalidatePasswordSession();
  }
}

function stageHasValue(mode: AuthMode) {
  return fieldsForMode(mode).some((field) => Boolean(fieldValue(field).trim()));
}

function stageStatus(mode: AuthMode) {
  const auth = props.draft.auth;
  if (mode === "password") {
    if (auth.loginUsername.trim() && auth.loginPassword.trim()) return "可切换";
    if (auth.loginUsername.trim()) return "账号已补全";
    return "可补全";
  }
  if (mode === "session") {
    if (auth.sessionCookie.trim()) return "已保存";
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
    props.draft.auth.sessionCookie = "";
    props.draft.auth.apiUser = "";
    clearTokenChain();
  }
  props.draft.auth.mode = mode;
}

function invalidatePasswordSession() {
  if (props.draft.auth.mode === "password") {
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
  return currentAuthMode.value?.label || "认证凭据";
}
</script>

<template>
  <div class="provider-form-page provider-credentials-page">
    <section class="provider-form-block provider-auth-picker-block">
      <header class="provider-form-block-header">
        <span class="provider-form-block-icon"><IconLock /></span>
        <div><strong>认证方式</strong></div>
        <span class="provider-form-block-meta">选择主凭据</span>
      </header>
      <div class="provider-form-block-body">
        <div class="provider-auth-mode-grid" role="radiogroup" aria-label="认证方式">
          <button
            v-for="mode in visibleAuthModes"
            :key="mode.mode"
            type="button"
            class="provider-auth-mode-option"
            :class="[`is-${mode.mode}`, { active: draft.auth.mode === mode.mode }]"
            :aria-checked="draft.auth.mode === mode.mode"
            :title="mode.description"
            role="radio"
            @click="selectMode(mode.mode)"
          >
            <span class="provider-auth-mode-icon">
              <ProviderAuthIcon :mode="mode.mode" :size="20" :decorative="true" />
            </span>
            <span class="provider-auth-mode-copy"><strong>{{ mode.label }}</strong></span>
            <IconCheckCircle v-if="draft.auth.mode === mode.mode" class="provider-auth-mode-check" />
          </button>
        </div>
      </div>
    </section>

    <section v-if="apiKeyOptions.length > 0" class="provider-form-block provider-api-key-vault-block">
      <header class="provider-form-block-header provider-api-key-vault-heading">
        <span class="provider-form-block-icon provider-form-block-icon-auth">
          <ProviderAuthIcon mode="apiKey" :protocol="draft.identity.protocol" :size="18" :decorative="true" />
        </span>
        <div><strong>API Key 密钥库</strong></div>
        <span class="provider-form-block-meta">{{ apiKeyOptions.length }} 个已保存</span>
      </header>
      <div class="provider-form-block-body provider-api-key-vault-body">
        <p class="provider-credential-inline-note provider-field-wide">
          这些 Key 都属于当前中转站卡片。选择其中一个后，会将它设为主 Key。
        </p>
        <ProviderApiKeyPicker
          class="provider-field-wide"
          :options="apiKeyOptions"
          :current-key="draft.auth.apiKey"
          :current-token-id="draft.auth.apiKeyTokenId"
          :remote-managed="currentProtocol?.capabilities.apiKeyManagement ?? false"
          :selectable="true"
          @select="emit('select-api-key', $event)"
        />
      </div>
    </section>

    <section class="provider-form-block provider-credential-active-panel">
      <header class="provider-form-block-header provider-credential-active-heading">
        <span class="provider-form-block-icon provider-form-block-icon-auth">
          <ProviderAuthIcon :mode="draft.auth.mode" :protocol="draft.identity.protocol" :size="18" :decorative="true" />
        </span>
        <div><strong>{{ activeLabel() }}</strong></div>
        <span class="provider-form-block-required">当前使用</span>
      </header>
      <div class="provider-form-block-body provider-field-grid">
        <ProviderCredentialFields
          :fields="activeFields"
          :required-fields="currentAuthMode?.requiredFields ?? []"
          :draft="draft"
          @copy-api-key="emit('copy-api-key')"
          @update-field="updateField"
        />
        <p v-if="currentAuthMode?.note" class="provider-credential-inline-note provider-field-wide">
          {{ currentAuthMode.note }}
        </p>
      </div>
    </section>

    <section v-if="secondaryModes.length > 0" class="provider-form-block provider-credential-chain">
      <header class="provider-form-block-header provider-credential-chain-heading">
        <span class="provider-form-block-icon provider-form-block-icon-neutral"><IconLock /></span>
        <div><strong>后续凭据</strong></div>
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
  </div>
</template>
