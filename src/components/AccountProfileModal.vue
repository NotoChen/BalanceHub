<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { IconGift, IconUser } from "@arco-design/web-vue/es/icon";
import type { Provider } from "../stores/providers";
import { providerDisplayLabel } from "../utils/provider-display";

const props = defineProps<{
  visible: boolean;
  provider: Provider | null;
  loading: boolean;
  error: string;
  redeemLoading: boolean;
  redeemError: string;
}>();

const emit = defineEmits<{
  "update:visible": [visible: boolean];
  submitProfile: [username: string, displayName: string];
  submitRedeem: [code: string];
}>();

const usernameDraft = ref("");
const displayNameDraft = ref("");
const codeDraft = ref("");

watch(
  [() => props.visible, () => props.provider?.identity.id],
  () => {
    usernameDraft.value = props.provider?.identity.username ?? "";
    displayNameDraft.value = props.provider?.identity.displayName ?? "";
    codeDraft.value = "";
  },
  { flush: "sync" },
);

const modalTitle = computed(() => {
  if (!props.provider) return "账号资料";
  const label = providerDisplayLabel(props.provider);
  if (props.provider.actions.editProfile && props.provider.actions.redeemCode) {
    return `${label} · 账号与兑换`;
  }
  if (props.provider.actions.redeemCode) return `${label} · 兑换余额`;
  return `${label} · 账号资料`;
});

const showProfile = computed(() => props.provider?.actions.editProfile ?? false);
const showRedeem = computed(() => props.provider?.actions.redeemCode ?? false);

function submitProfile() {
  emit("submitProfile", usernameDraft.value, displayNameDraft.value);
}

function submitRedeem() {
  emit("submitRedeem", codeDraft.value);
}
</script>

<template>
  <a-modal
    :visible="visible"
    modal-class="surface-modal account-profile-modal"
    :footer="false"
    width="min(560px, calc(100vw - 32px))"
    unmount-on-close
    @update:visible="emit('update:visible', $event)"
  >
    <template #title>
      <div class="surface-modal-title">
        <span class="surface-modal-title-icon"><icon-user /></span>
        <span class="surface-modal-title-copy">
          <strong>{{ modalTitle }}</strong>
        </span>
      </div>
    </template>

    <div class="account-profile-panel">
      <section v-if="showProfile" class="account-profile-section">
        <h3>站点资料</h3>
        <p class="account-profile-hint">
          用户名同时用于站点登录；修改后请使用新用户名登录。邮箱、二次验证和第三方绑定需要站点自己的验证流程，请在站点页面操作。
        </p>
        <div class="account-profile-fields">
          <label>
            <span>用户名</span>
            <a-input
              v-model="usernameDraft"
              placeholder="站点登录用户名"
              allow-clear
              @press-enter="submitProfile"
            />
          </label>
          <label>
            <span>显示名称</span>
            <a-input
              v-model="displayNameDraft"
              placeholder="留空则显示用户名"
              allow-clear
              @press-enter="submitProfile"
            />
          </label>
        </div>
        <a-alert v-if="error" type="error" show-icon>
          <span>{{ error }}</span>
        </a-alert>
        <div class="account-profile-actions">
          <a-button type="primary" :loading="loading" @click="submitProfile">
            保存资料
          </a-button>
        </div>
      </section>

      <section v-if="showRedeem" class="account-profile-section">
        <h3><icon-gift /> 兑换余额</h3>
        <p class="account-profile-hint">
          输入站点生成的兑换码。兑换成功后会立即刷新余额和订阅窗口。
        </p>
        <div class="account-profile-redeem">
          <a-input
            v-model="codeDraft"
            placeholder="兑换码"
            allow-clear
            @press-enter="submitRedeem"
          />
          <a-button type="primary" :loading="redeemLoading" @click="submitRedeem">
            兑换
          </a-button>
        </div>
        <a-alert v-if="redeemError" type="error" show-icon>
          <span>{{ redeemError }}</span>
        </a-alert>
      </section>
    </div>
  </a-modal>
</template>
