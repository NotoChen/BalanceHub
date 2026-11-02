<script setup lang="ts">
import { computed } from "vue";
import { IconDelete, IconPlus } from "@arco-design/web-vue/es/icon";
import type { ProviderInput, ProviderProtocolDescriptor } from "../../stores/providers";
import { providerProtocolDescriptor } from "../../utils/provider-protocol";
import ProviderEndpointCatalog from "./ProviderEndpointCatalog.vue";
import ProviderHeaderListEditor from "./ProviderHeaderListEditor.vue";

const props = defineProps<{
  draft: ProviderInput;
  providerProtocols: ProviderProtocolDescriptor[];
}>();

const descriptor = computed(() =>
  providerProtocolDescriptor(
    props.providerProtocols,
    props.draft.identity.protocol,
  ),
);
const capabilities = computed(() => descriptor.value?.capabilities);
const endpointLimit = computed(
  () => capabilities.value?.maxEndpointOverrides ?? 0,
);
const endpoints = computed(() => descriptor.value?.endpoints ?? []);

/// 备用地址只对经过中转站传输层的协议生效；数量上限由后端给出，存储层会
/// 截断超出的地址，这里提前挡住，避免填了却存不进去。
const backupUrlLimit = computed(() => capabilities.value?.maxBackupUrls ?? 0);
const backupUrlsSupported = computed(() => backupUrlLimit.value > 0);

function addBackupUrl() {
  if (props.draft.identity.backupUrls.length >= backupUrlLimit.value) return;
  props.draft.identity.backupUrls.push("");
}

const authSchemeOptions = [
  { label: "Authorization: Bearer", value: "bearer" },
  { label: "X-Api-Key", value: "xApiKey" },
  { label: "不自动附加凭据", value: "none" },
];
</script>

<template>
  <div class="provider-form-page">
    <section v-if="backupUrlsSupported" class="provider-form-section">
      <h3 class="provider-form-section-title">
        备用地址
        <span class="provider-backup-url-actions">
          <span v-if="draft.identity.backupUrls.length" class="provider-backup-url-count">
            {{ draft.identity.backupUrls.length }} / {{ backupUrlLimit }}
          </span>
          <a-button
            type="text"
            size="small"
            :disabled="draft.identity.backupUrls.length >= backupUrlLimit"
            @click="addBackupUrl"
          >
            <template #icon><IconPlus /></template>添加地址
          </a-button>
        </span>
      </h3>
      <div v-if="draft.identity.backupUrls.length" class="provider-backup-url-list">
        <div v-for="(_, index) in draft.identity.backupUrls" :key="index" class="provider-backup-url-row">
          <span class="provider-backup-url-index">{{ index + 1 }}</span>
          <a-input v-model="draft.identity.backupUrls[index]" :aria-label="`备用地址 ${index + 1}`" placeholder="https://backup.example.com" allow-clear />
          <a-button type="text" status="danger" :aria-label="`删除备用地址 ${index + 1}`" @click="draft.identity.backupUrls.splice(index, 1)"><template #icon><IconDelete /></template></a-button>
        </div>
      </div>
      <p v-else class="provider-empty-note">尚未添加备用地址</p>
    </section>
    <section v-if="capabilities?.requestHeaders" class="provider-form-section">
      <h3 class="provider-form-section-title">请求约定</h3>
      <div class="provider-form-section-body">
        <a-form-item class="provider-field" label="额外请求头">
          <ProviderHeaderListEditor v-model="draft.request.headers" />
          <template #extra>
            附加到该中转站的每个请求；名称或值为空的行不会发送。
          </template>
        </a-form-item>
        <a-form-item
          v-if="capabilities?.apiKeyScheme"
          class="provider-field"
          label="API Key 发送方式"
        >
          <a-select
            v-model="draft.request.authScheme"
            :options="authSchemeOptions"
          />
          <template #extra>
            仅用于模型列表、余额接口等协议级查询；选择「不自动附加凭据」时请用
            请求头自行提供认证。
          </template>
        </a-form-item>
      </div>
    </section>
    <section
      v-if="endpointLimit > 0 && endpoints.length"
      class="provider-form-section"
    >
      <h3 class="provider-form-section-title">接口目录</h3>
      <div class="provider-form-section-body">
        <ProviderEndpointCatalog
          :draft="draft"
          :endpoints="endpoints"
          :limit="endpointLimit"
        />
      </div>
    </section>
  </div>
</template>
