<script setup lang="ts">
import {
  IconApps,
  IconCheckCircleFill,
  IconPlus,
  IconSync,
  IconUpload,
} from "@arco-design/web-vue/es/icon";
import { useHostCapabilities } from "../composables/useHostCapabilities";

const { agentAvailable, ensure } = useHostCapabilities();
void ensure();

defineProps<{
  visible: boolean;
  providerCount: number;
  cliConfigured: boolean;
  cliDetecting: boolean;
  importingAppData: boolean;
}>();

const emit = defineEmits<{
  addProvider: [];
  importData: [];
  probeCli: [];
  openAgentWorkspace: [];
  /// 关闭窗口只是稍后再看，不应把「完成初始化」永久写入设置。
  later: [];
  finish: [];
}>();
</script>

<template>
  <a-modal
    :visible="visible"
    :footer="false"
    :width="700"
    unmount-on-close
    modal-class="surface-modal onboarding-modal"
    @update:visible="(value) => { if (!value) emit('later'); }"
  >
    <div class="onboarding-panel">
      <div class="onboarding-header">
        <div class="onboarding-logo">BH</div>
        <div>
          <h2>初始化 BalanceHub</h2>
          <p v-if="agentAvailable">先完成账号来源和本机环境配置，之后就可以进入主界面。</p>
          <p v-else>先配置一个中转站，之后就可以进入主界面。</p>
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

        <section v-if="agentAvailable" class="onboarding-step">
          <div class="onboarding-step-index">
            <icon-check-circle-fill v-if="cliConfigured" />
            <span v-else>2</span>
          </div>
          <div>
            <h3>本机测活环境</h3>
            <p v-if="cliConfigured">已检测到可用的 CLI 环境。</p>
            <p v-else>可以检测本机已安装的 Agent CLI；检测结果会用于测活和临时启动。</p>
            <div class="onboarding-actions">
              <a-button :loading="cliDetecting" @click="emit('probeCli')">
                <template #icon><icon-sync /></template>
                {{ cliDetecting ? "正在检测…" : "检测本机 CLI" }}
              </a-button>
              <a-button @click="emit('openAgentWorkspace')">
                <template #icon><icon-apps /></template>
                打开 Agent 工作区
              </a-button>
            </div>
          </div>
        </section>
      </div>

      <div class="onboarding-footer">
        <span>关闭窗口只会在本次会话隐藏，下次启动仍可继续配置。</span>
        <div>
          <a-button @click="emit('later')">稍后配置</a-button>
          <a-button type="primary" @click="emit('finish')">进入应用</a-button>
        </div>
      </div>
    </div>
  </a-modal>
</template>
