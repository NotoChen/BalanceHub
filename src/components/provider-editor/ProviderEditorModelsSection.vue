<script setup lang="ts">
import { computed } from "vue";
import type { SelectOptionData } from "@arco-design/web-vue";
import type { ProviderInput, ProviderProtocolDescriptor } from "../../stores/providers";
import { providerProtocolDescriptor } from "../../utils/provider-protocol";
import ProviderEditorModelConfigSection from "./ProviderEditorModelConfigSection.vue";

const props = defineProps<{
  draft: ProviderInput;
  availableModels: string[];
  providerProtocols: ProviderProtocolDescriptor[];
}>();

/// 自定义模型列表地址只对通用 OpenAI 兼容协议生效；其它协议的模型接口由
/// 协议自己决定，填了也不会被读取。
const modelsUrlSupported = computed(
  () =>
    providerProtocolDescriptor(props.providerProtocols, props.draft.identity.protocol)
      ?.capabilities.modelsUrl ?? false,
);

function modelChoices(selectedModel: string) {
  const values = [...props.availableModels, selectedModel || ""]
    .map((model) => model.trim())
    .filter(Boolean);
  return [...new Set(values)].map((model) => ({ label: model, value: model }));
}

const modelOptions = computed(() => modelChoices(props.draft.cli.preferredModel));

function filterModelOption(inputValue: string, option: SelectOptionData) {
  const query = inputValue.trim().toLocaleLowerCase();
  if (!query) return true;
  return String(option.label ?? option.value ?? "").toLocaleLowerCase().includes(query);
}
</script>

<template>
  <ProviderEditorModelConfigSection
    :draft="draft"
    :available-models="availableModels"
    :show-models-url="modelsUrlSupported"
  />
  <section class="provider-form-section">
    <h3 class="provider-form-section-title">临时 CLI</h3>
    <div class="provider-form-section-body">
      <a-form-item class="provider-field" label="首选模型">
        <a-select
          v-model="draft.cli.preferredModel"
          :options="modelOptions"
          allow-search
          allow-create
          allow-clear
          :filter-option="filterModelOption"
          placeholder="搜索模型或直接输入"
        />
        <template #extra>{{ availableModels.length ? `已获取 ${availableModels.length} 个模型` : "暂未获取模型，可直接输入" }}</template>
      </a-form-item>
    </div>
  </section>
</template>
