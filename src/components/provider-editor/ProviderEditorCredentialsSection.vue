<script setup lang="ts">
import { computed } from "vue";
import { IconCheckCircle, IconCopy, IconLock, IconRight } from "@arco-design/web-vue/es/icon";
import type { AuthMode, ProviderInput } from "../../stores/providers";
import ProviderAuthIcon from "../ProviderAuthIcon.vue";
import ProviderApiKeyPicker from "./ProviderApiKeyPicker.vue";

const props = defineProps<{
  draft: ProviderInput;
  apiKeyOptions: ProviderInput["auth"]["apiKeyOptions"];
}>();

const emit = defineEmits<{
  "copy-api-key": [];
  "select-api-key": [option: ProviderInput["auth"]["apiKeyOptions"][number]];
}>();

const authModes: { value: AuthMode; label: string; description: string }[] = [
  { value: "password", label: "账号密码", description: "登录并建立会话" },
  { value: "session", label: "Cookie", description: "已有浏览器会话" },
  { value: "accessToken", label: "访问令牌", description: "账号接口令牌" },
  { value: "apiKey", label: "API Key", description: "仅密钥额度" },
];
const supplementModes: Record<AuthMode, AuthMode[]> = {
  password: ["session", "accessToken", "apiKey"],
  session: ["accessToken", "apiKey"],
  accessToken: ["apiKey"],
  apiKey: [],
};

const apiUserPlaceholder = computed(() =>
  props.draft.auth.mode === "session" ? "自动解析，也可手动填写" : "输入用户 ID",
);

const secondaryModes = computed(() => {
  return supplementModes[props.draft.auth.mode] || [];
});

const secondaryOrderText = computed(() => secondaryModes.value.map(stageLabel).join(" → "));

function stageLabel(mode: AuthMode) {
  return authModes.find((item) => item.value === mode)?.label || mode;
}

function stageHasValue(mode: AuthMode) {
  const auth = props.draft.auth;
  if (mode === "password") return Boolean(auth.loginUsername.trim() && auth.loginPassword.trim());
  if (mode === "session") return Boolean(auth.sessionCookie.trim());
  if (mode === "accessToken") return Boolean(auth.accessToken.trim());
  return Boolean(auth.apiKey.trim());
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
  if (mode === props.draft.auth.mode) {
    return;
  }

  // 切入账号密码时强制重新登录；从账号密码切到下游认证时保留已建立的会话，
  // 这样用户不需要再次粘贴 Cookie。
  if (mode === "password" && props.draft.auth.mode !== "password") {
    props.draft.auth.sessionCookie = "";
    props.draft.auth.apiUser = "";
  }
  props.draft.auth.mode = mode;
}

function invalidatePasswordSession() {
  if (props.draft.auth.mode === "password") {
    props.draft.auth.sessionCookie = "";
    props.draft.auth.apiUser = "";
  }
}

function syncApiKeySelection() {
  const current = props.draft.auth.apiKey.trim();
  props.draft.auth.apiKeyTokenId =
    props.apiKeyOptions.find((option) => option.key.trim() === current)?.tokenId || "";
}

function activeLabel() {
  return authModes.find((item) => item.value === props.draft.auth.mode)?.label || "认证凭据";
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
            v-for="mode in authModes"
            :key="mode.value"
            type="button"
            class="provider-auth-mode-option"
            :class="[`is-${mode.value}`, { active: draft.auth.mode === mode.value }]"
            :aria-checked="draft.auth.mode === mode.value"
            :title="mode.description"
            role="radio"
            @click="selectMode(mode.value)"
          >
            <span class="provider-auth-mode-icon">
              <ProviderAuthIcon :mode="mode.value" :size="20" :decorative="true" />
            </span>
            <span class="provider-auth-mode-copy"><strong>{{ mode.label }}</strong></span>
            <IconCheckCircle v-if="draft.auth.mode === mode.value" class="provider-auth-mode-check" />
          </button>
        </div>
      </div>
    </section>

    <section class="provider-form-block provider-credential-active-panel">
      <header class="provider-form-block-header provider-credential-active-heading">
        <span class="provider-form-block-icon provider-form-block-icon-auth">
          <ProviderAuthIcon :mode="draft.auth.mode" :size="18" :decorative="true" />
        </span>
        <div><strong>{{ activeLabel() }}</strong></div>
        <span class="provider-form-block-required">当前使用</span>
      </header>
      <div class="provider-form-block-body provider-field-grid">
        <a-form-item
          v-if="draft.auth.mode === 'session'"
          class="provider-field provider-field-wide"
          field="auth.sessionCookie"
          label="会话 Cookie"
          required
        >
          <a-input-password v-model="draft.auth.sessionCookie" placeholder="session=xxx 或直接粘贴 Cookie 值" allow-clear />
        </a-form-item>

        <a-form-item v-if="draft.auth.mode === 'accessToken'" class="provider-field" field="auth.accessToken" label="访问令牌" required>
          <a-input-password v-model="draft.auth.accessToken" placeholder="粘贴访问令牌" allow-clear />
        </a-form-item>

        <a-form-item
          v-if="draft.auth.mode === 'session' || draft.auth.mode === 'accessToken'"
          class="provider-field"
          field="auth.apiUser"
          label="API User ID"
          :required="draft.auth.mode === 'accessToken'"
        >
          <a-input v-model="draft.auth.apiUser" :placeholder="apiUserPlaceholder" allow-clear />
        </a-form-item>

        <a-form-item v-if="draft.auth.mode === 'apiKey'" class="provider-field provider-field-wide" field="auth.apiKey" label="API Key" required>
          <div class="input-action-row">
            <a-input-password
              v-model="draft.auth.apiKey"
              placeholder="sk-..."
              allow-clear
              @update:model-value="syncApiKeySelection"
            />
            <a-button :disabled="!draft.auth.apiKey.trim()" aria-label="复制 API Key" @click="emit('copy-api-key')">
              <template #icon><IconCopy /></template>
            </a-button>
          </div>
        </a-form-item>
        <ProviderApiKeyPicker
          v-if="draft.auth.mode === 'apiKey' && apiKeyOptions.length > 0"
          class="provider-field-wide"
          :options="apiKeyOptions"
          :current-key="draft.auth.apiKey"
          :current-token-id="draft.auth.apiKeyTokenId"
          :selectable="apiKeyOptions.length > 1"
          @select="emit('select-api-key', $event)"
        />
        <a-form-item v-if="draft.auth.mode === 'password'" class="provider-field" field="auth.loginUsername" label="账号" required>
          <a-input
            v-model="draft.auth.loginUsername"
            placeholder="用户名或邮箱"
            allow-clear
            @update:model-value="invalidatePasswordSession"
          />
        </a-form-item>
        <a-form-item v-if="draft.auth.mode === 'password'" class="provider-field" field="auth.loginPassword" label="密码" required>
          <a-input-password
            v-model="draft.auth.loginPassword"
            placeholder="NewAPI 登录密码"
            allow-clear
            @update:model-value="invalidatePasswordSession"
          />
        </a-form-item>
        <a-form-item v-if="draft.auth.mode === 'password' && draft.auth.apiUser" class="provider-field provider-field-wide" field="auth.apiUser" label="登录后用户 ID">
          <a-input v-model="draft.auth.apiUser" readonly />
        </a-form-item>
        <p v-if="draft.auth.mode === 'password'" class="provider-credential-inline-note provider-field-wide">
          保存后首次同步会登录站点并缓存会话；开启 2FA 或验证码的站点请改用 Cookie。
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
          :key="mode"
          class="provider-credential-stage"
          :class="[`is-${mode}`, { 'has-value': stageHasValue(mode) }]"
          :open="mode === 'apiKey' && apiKeyOptions.length > 0"
        >
          <summary>
            <span class="provider-credential-stage-main">
              <span class="provider-credential-stage-icon">
                <ProviderAuthIcon :mode="mode" :size="16" :decorative="true" />
              </span>
              <strong>{{ stageLabel(mode) }}</strong>
            </span>
            <span class="provider-credential-stage-status" :class="stageStatusClass(mode)">
              {{ stageStatus(mode) }}
            </span>
            <IconRight class="provider-credential-stage-chevron" />
          </summary>

          <div v-if="mode === 'session'" class="provider-credential-stage-fields provider-field-grid">
            <a-form-item class="provider-field provider-field-wide" field="auth.sessionCookie" label="会话 Cookie">
              <a-input-password v-model="draft.auth.sessionCookie" placeholder="session=xxx" allow-clear />
            </a-form-item>
          </div>

          <div v-else-if="mode === 'accessToken'" class="provider-credential-stage-fields provider-field-grid">
            <a-form-item class="provider-field provider-field-wide" field="auth.accessToken" label="访问令牌">
              <a-input-password v-model="draft.auth.accessToken" placeholder="自动获取或手动填写" allow-clear />
            </a-form-item>
          </div>

          <div v-else class="provider-credential-stage-fields provider-field-grid">
            <a-form-item class="provider-field provider-field-wide" field="auth.apiKey" label="API Key">
              <div class="input-action-row">
                <a-input-password
                  v-model="draft.auth.apiKey"
                  placeholder="自动获取或手动填写"
                  allow-clear
                  @update:model-value="syncApiKeySelection"
                />
                <a-button :disabled="!draft.auth.apiKey.trim()" aria-label="复制 API Key" @click="emit('copy-api-key')">
                  <template #icon><IconCopy /></template>
                </a-button>
              </div>
            </a-form-item>
            <ProviderApiKeyPicker
              v-if="apiKeyOptions.length > 0"
              class="provider-field-wide"
              :options="apiKeyOptions"
              :current-key="draft.auth.apiKey"
              :current-token-id="draft.auth.apiKeyTokenId"
              :selectable="apiKeyOptions.length > 1"
              @select="emit('select-api-key', $event)"
            />
          </div>
        </details>
      </div>
    </section>
  </div>
</template>
