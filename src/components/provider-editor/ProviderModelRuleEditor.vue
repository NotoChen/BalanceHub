<script setup lang="ts">
import { computed, ref } from "vue";
import type { TagData } from "@arco-design/web-vue";
import { toggleExactModelRule } from "../../utils/model-rule-input";

const props = defineProps<{
  modelValue: string[];
  /// 站点最近一次同步到的原始模型列表；规则会写进草稿，不受当前展示规则影响。
  candidates: string[];
  placeholder: string;
}>();

const emit = defineEmits<{
  "update:modelValue": [rules: string[]];
}>();

const search = ref("");
const visible = ref(false);

const models = computed(() => {
  const query = search.value.trim().toLocaleLowerCase();
  const selected = new Set(props.modelValue);
  return [...new Set(props.candidates.map((model) => model.trim()).filter(Boolean))]
    .filter((model) => !query || model.toLocaleLowerCase().includes(query))
    .sort(
      (left, right) =>
        Number(selected.has(right)) - Number(selected.has(left)) ||
        left.localeCompare(right),
    )
    .slice(0, 300);
});

function isSelected(model: string) {
  return props.modelValue.includes(model);
}

function toggle(model: string) {
  emit("update:modelValue", toggleExactModelRule(props.modelValue, model));
}

/// InputTag 的回显可能是纯文本或标签对象；规则只保存模型 ID 字符串。
function updateRules(value: (string | number | TagData)[]) {
  emit(
    "update:modelValue",
    value
      .map((item) =>
        String(typeof item === "object" ? item.value ?? "" : item).trim(),
      )
      .filter(Boolean),
  );
}
</script>

<template>
  <div class="provider-model-rule">
    <a-input-tag
      :model-value="modelValue"
      :placeholder="placeholder"
      allow-clear
      @update:model-value="updateRules"
    />
    <a-popover
      v-model:visible="visible"
      trigger="click"
      position="br"
      :content-style="{ padding: '10px' }"
    >
      <a-button size="small" :disabled="!candidates.length">从站点模型选择</a-button>
      <template #content>
        <div class="provider-model-picker">
          <a-input
            v-model="search"
            size="small"
            placeholder="搜索模型"
            allow-clear
          />
          <div v-if="models.length" class="provider-model-picker-list">
            <div
              v-for="model in models"
              :key="model"
              class="provider-model-picker-item"
              @click="toggle(model)"
            >
              <a-checkbox
                :model-value="isSelected(model)"
                @click.stop
                @change="toggle(model)"
              />
              <code>{{ model }}</code>
            </div>
          </div>
          <p v-else class="provider-model-picker-empty">
            站点暂无模型列表，可以直接输入规则。
          </p>
          <p class="provider-model-picker-note">
            勾选写入精确模型 ID；通配符和 re: 正则仍需直接输入。
          </p>
        </div>
      </template>
    </a-popover>
  </div>
</template>
