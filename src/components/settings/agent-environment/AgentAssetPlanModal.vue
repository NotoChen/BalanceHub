<script setup lang="ts">
import { IconLoading } from "@arco-design/web-vue/es/icon";
import type { AgentAssetPlan } from "../../../stores/provider-types";
import ContentChange from "../../ContentChange.vue";

defineProps<{
  visible: boolean;
  plan: AgentAssetPlan | null;
  preparing: boolean;
  error: string;
  expired: boolean;
  canApply: boolean;
  affectedAssets: { id: string; label: string }[];
  affectedInstallations: { id: string; label: string; path: string | null }[];
}>();
const emit = defineEmits<{ close: []; confirm: []; retry: [] }>();
</script>

<template>
  <a-modal :visible="visible" width="min(960px, calc(100vw - 32px))" modal-class="surface-modal agent-asset-plan-modal" title-align="start" closable mask-closable esc-to-close unmount-on-close :footer="false" @update:visible="(value: boolean) => !value && emit('close')">
    <template #title>{{ plan?.title || "准备资产变更计划" }}</template>
    <div class="agent-asset-plan">
      <p v-if="preparing" class="agent-asset-operation" role="status"><IconLoading class="agent-version-loading" /> 正在检查当前配置与原生机制</p>
      <p v-if="error" class="agent-environment-stale-error" role="status">{{ error }}</p>
      <template v-if="plan">
        <p class="agent-asset-detail-note">确认后会再次核对当前配置，并检查{{ plan.action === 'remove' ? '资产是否已从 Agent 卸载' : '更改后的状态' }}。</p>
        <div class="agent-asset-plan-changes"><ContentChange v-for="(change, index) in plan.changes" :key="index" v-bind="change" /></div>
        <section v-if="affectedAssets.length" class="agent-asset-plan-impact"><strong>已知受影响资产 · {{ affectedAssets.length }}</strong><ul><li v-for="asset in affectedAssets" :key="asset.id">{{ asset.label }}</li></ul></section>
        <section v-if="affectedInstallations.length" class="agent-asset-plan-impact"><strong>{{ affectedInstallations.length > 1 ? "共享此配置的安装" : "关联安装" }}</strong><ul><li v-for="installation in affectedInstallations" :key="installation.id">{{ installation.label }}<code v-if="installation.path">{{ installation.path }}</code></li></ul></section>
        <p v-if="plan.reloadEffect" class="agent-asset-detail-note">生效方式：{{ plan.reloadEffect }}</p>
        <p v-if="plan.trustEffect" class="agent-asset-detail-note">信任影响：{{ plan.trustEffect }}</p>
        <p v-if="expired" class="agent-environment-stale-error" role="status">计划已过期，请重新预览。</p>
        <p v-else class="agent-asset-detail-note">有效至 {{ new Date(plan.expiresAt).toLocaleTimeString() }}</p>
      </template>
      <div class="agent-asset-modal-actions"><a-button @click="emit('close')">取消</a-button><a-button v-if="error || expired" :loading="preparing" @click="emit('retry')">重新预览</a-button><a-button type="primary" :status="plan?.action === 'remove' ? 'danger' : 'normal'" :disabled="!canApply" @click="emit('confirm')">{{ plan?.action === 'remove' ? '确认卸载' : '确认应用' }}</a-button></div>
    </div>
  </a-modal>
</template>
