<script setup lang="ts">
import { computed } from "vue";
import { IconCloud, IconDelete, IconLink, IconPlus, IconRefresh } from "@arco-design/web-vue/es/icon";
import type { ProviderInput, ProviderSiteProbeResult } from "../../stores/providers";

const props = defineProps<{
  draft: ProviderInput;
  siteProbeResult: ProviderSiteProbeResult | null;
  probingSite: boolean;
  siteNameSourceBaseUrl: string;
}>();

const emit = defineEmits<{
  "probe-site": [];
}>();

const normalizedBaseUrl = computed(() => normalizeBaseUrl(props.draft.identity.baseUrl));
const detectionIsCurrent = computed(() =>
  Boolean(props.draft.identity.name.trim() && normalizedBaseUrl.value === props.siteNameSourceBaseUrl),
);
const hostFallback = computed(() => {
  try {
    return new URL(props.draft.identity.baseUrl.trim()).host || "等待识别";
  } catch {
    return props.draft.identity.baseUrl.trim().replace(/^https?:\/\//i, "").split("/")[0] || "等待识别";
  }
});
const detectedName = computed(() => {
  const name = props.draft.identity.name.trim();
  return name && normalizedBaseUrl.value === props.siteNameSourceBaseUrl ? name : hostFallback.value;
});
const detectionLabel = computed(() => {
  if (props.probingSite) return "识别中";
  if (detectionIsCurrent.value) return "已识别";
  return "待识别";
});

function normalizeBaseUrl(value: string) {
  return value.trim().replace(/\/+$/, "");
}

function addBackupUrl() {
  props.draft.identity.backupUrls.push("");
}

function removeBackupUrl(index: number) {
  props.draft.identity.backupUrls.splice(index, 1);
}
</script>

<template>
  <div class="provider-form-page provider-basics-page">
    <section class="provider-form-block provider-form-block-primary">
      <header class="provider-form-block-header">
        <span class="provider-form-block-icon"><IconCloud /></span>
        <div><strong>中转站</strong></div>
        <span class="provider-form-block-required">地址必填</span>
      </header>
      <div class="provider-form-block-body">
        <a-form-item class="provider-field" field="identity.baseUrl" label="主站地址" required>
          <a-input
            v-model="draft.identity.baseUrl"
            placeholder="https://relay.example.com"
            allow-clear
            @blur="emit('probe-site')"
          >
            <template #prefix><IconLink /></template>
            <template #suffix>
              <a-tooltip content="重新识别站点">
                <button
                  type="button"
                  class="provider-inline-icon-button"
                  :class="{ spinning: probingSite }"
                  aria-label="重新识别站点"
                  @click.stop="emit('probe-site')"
                >
                  <IconRefresh />
                </button>
              </a-tooltip>
            </template>
          </a-input>
        </a-form-item>

        <div class="provider-site-detection" :class="{ loading: probingSite, ready: detectionLabel === '已识别' }">
          <span class="provider-site-detection-mark"><IconCloud /></span>
          <div class="provider-site-detection-copy">
            <strong>{{ detectedName }}</strong>
            <span>NewAPI · {{ detectionLabel }}</span>
          </div>
          <span v-if="detectionIsCurrent && siteProbeResult?.message" class="provider-site-detection-message">{{ siteProbeResult.message }}</span>
        </div>
      </div>
    </section>

    <section class="provider-form-block">
      <header class="provider-form-block-header">
        <span class="provider-form-block-icon provider-form-block-icon-neutral"><IconLink /></span>
        <div><strong>备用地址</strong></div>
        <span class="provider-form-block-meta">仅维护</span>
        <a-button type="text" size="small" @click="addBackupUrl">
          <template #icon><IconPlus /></template>
          添加地址
        </a-button>
      </header>
      <div class="provider-form-block-body">
        <div v-if="draft.identity.backupUrls.length" class="provider-backup-url-list">
          <div
            v-for="(_, index) in draft.identity.backupUrls"
            :key="`backup-url-${index}`"
            class="provider-backup-url-row"
          >
            <span class="provider-backup-url-index">{{ index + 1 }}</span>
            <a-input
              v-model="draft.identity.backupUrls[index]"
              :placeholder="`https://backup-${index + 1}.example.com`"
              allow-clear
            />
            <a-button type="text" status="danger" aria-label="删除备用地址" @click="removeBackupUrl(index)">
              <template #icon><IconDelete /></template>
            </a-button>
          </div>
        </div>
        <button v-else type="button" class="provider-backup-empty" @click="addBackupUrl">
          <IconPlus />
          <span><strong>添加备用地址</strong></span>
        </button>
      </div>
    </section>
  </div>
</template>
