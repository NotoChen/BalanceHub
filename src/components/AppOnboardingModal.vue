<script setup lang="ts">
import {
  IconCheckCircleFill,
  IconPlus,
  IconSettings,
  IconUpload,
} from "@arco-design/web-vue/es/icon";

defineProps<{
  visible: boolean;
  providerCount: number;
  cliConfigured: boolean;
  importingAppData: boolean;
}>();

const emit = defineEmits<{
  addProvider: [];
  importData: [];
  openSettings: [];
  finish: [];
}>();
</script>

<template>
  <a-modal
    :visible="visible"
    :footer="false"
    :width="700"
    unmount-on-close
    modal-class="onboarding-modal"
    @update:visible="(value) => { if (!value) emit('finish'); }"
  >
    <div class="onboarding-panel">
      <div class="onboarding-header">
        <div class="onboarding-logo">BH</div>
        <div>
          <h2>初始化 BalanceHub</h2>
          <p>先完成账号来源和本机环境配置，之后就可以进入主界面。</p>
        </div>
      </div>

      <div class="onboarding-steps">
        <section class="onboarding-step">
          <div class="onboarding-step-index">
            <icon-check-circle-fill v-if="providerCount > 0" />
            <span v-else>1</span>
          </div>
          <div>
            <h3>中转站配置</h3>
            <p v-if="providerCount > 0">已配置 {{ providerCount }} 个中转站。</p>
            <p v-else>可以导入已有配置，也可以新建第一个中转站。</p>
            <div class="onboarding-actions">
              <a-button :loading="importingAppData" @click="emit('importData')">
                <template #icon><icon-upload /></template>
                导入配置
              </a-button>
              <a-button type="primary" @click="emit('addProvider')">
                <template #icon><icon-plus /></template>
                添加中转站
              </a-button>
            </div>
          </div>
        </section>

        <section class="onboarding-step">
          <div class="onboarding-step-index">
            <icon-check-circle-fill v-if="cliConfigured" />
            <span v-else>2</span>
          </div>
          <div>
            <h3>本机测活环境</h3>
            <p v-if="cliConfigured">已检测到可用的 CLI 环境。</p>
            <p v-else>进入应用后会分别检测 Agent 与终端。</p>
            <div class="onboarding-actions">
              <a-button @click="emit('openSettings')">
                <template #icon><icon-settings /></template>
                打开设置
              </a-button>
            </div>
          </div>
        </section>
      </div>

      <div class="onboarding-footer">
        <a-button @click="emit('finish')">跳过引导</a-button>
        <a-button type="primary" @click="emit('finish')">进入应用</a-button>
      </div>
    </div>
  </a-modal>
</template>
