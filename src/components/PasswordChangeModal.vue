<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { Message } from "@arco-design/web-vue";
import type { Provider } from "../stores/providers";

const props = defineProps<{
  visible: boolean;
  provider: Provider | null;
  loading: boolean;
}>();

const emit = defineEmits<{
  "update:visible": [visible: boolean];
  submit: [originalPassword: string, password: string];
}>();

const originalPassword = ref("");
const password = ref("");
const confirmPassword = ref("");

const modalTitle = computed(() =>
  props.provider ? `${props.provider.identity.name} · 修改密码` : "修改密码",
);

watch(
  () => props.visible,
  (visible) => {
    if (!visible) {
      originalPassword.value = "";
      password.value = "";
      confirmPassword.value = "";
    }
  },
);

function submit() {
  if (!password.value.trim()) {
    Message.warning("请输入新密码");
    return;
  }
  if (password.value !== confirmPassword.value) {
    Message.warning("两次输入的新密码不一致");
    return;
  }
  emit("submit", originalPassword.value, password.value);
}
</script>

<template>
  <a-modal
    :visible="visible"
    :title="modalTitle"
    :footer="false"
    unmount-on-close
    @update:visible="emit('update:visible', $event)"
  >
    <div class="password-change-form">
      <label>
        <span>原密码</span>
        <a-input-password
          v-model="originalPassword"
          allow-clear
          autocomplete="current-password"
          placeholder="已有密码账号必填"
        />
        <small>已有密码账号必须填写；第三方登录且未设置过密码的账号可留空尝试。</small>
      </label>
      <label>
        <span>新密码</span>
        <a-input-password v-model="password" allow-clear autocomplete="new-password" />
      </label>
      <label>
        <span>确认新密码</span>
        <a-input-password v-model="confirmPassword" allow-clear autocomplete="new-password" />
      </label>
      <div class="password-change-actions">
        <a-button :disabled="loading" @click="emit('update:visible', false)">取消</a-button>
        <a-button type="primary" :loading="loading" @click="submit">更新密码</a-button>
      </div>
    </div>
  </a-modal>
</template>
