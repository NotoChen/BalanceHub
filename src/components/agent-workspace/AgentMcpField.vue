<script setup lang="ts">
import { useId } from "vue";
import { Plus, X } from "@lucide/vue";
import type { McpField, McpFieldDraft } from "../../utils/mcp-form-fields";
const props = defineProps<{ field: McpField; modelValue: McpFieldDraft; required?: boolean; error?: string; supportNote?: string }>();
const emit = defineEmits<{ "update:modelValue": [value: McpFieldDraft] }>();
const id = useId();
function patch(value: Partial<McpFieldDraft>) { emit("update:modelValue", { ...props.modelValue, ...value }); }
function item(index: number, value: string) { patch({ items: props.modelValue.items.map((old, at) => at === index ? value : old) }); }
function pair(index: number, key: "name" | "value", value: string) { patch({ pairs: props.modelValue.pairs.map((old, at) => at === index ? { ...old, [key]: value } : old) }); }
</script>
<template>
  <div class="mcp-field" :class="{ 'has-error': error }">
    <header><label :for="id">{{ field.label }}<span v-if="required" class="mcp-required">必填</span></label><span v-if="field.kind === 'list' || field.kind === 'pairs'" class="mcp-field-key">{{ field.key }}</span></header>
    <p :id="`${id}-help`" class="mcp-help">{{ field.help }}</p>
    <p v-if="supportNote" class="mcp-support-note">{{ supportNote }}</p>
    <template v-if="field.kind === 'list'">
      <div v-for="(value, index) in modelValue.items" :key="index" class="mcp-list-row">
        <span class="mcp-row-index">{{ index + 1 }}</span><a-input :aria-invalid="Boolean(error)" :aria-describedby="`${id}-help ${id}-error`" :id="index === 0 ? id : undefined" :model-value="value" :placeholder="field.placeholder" :aria-label="`${field.label} ${index + 1}`" @update:model-value="item(index, $event)" />
        <a-button type="text" :aria-label="`移除${field.label} ${index + 1}`" @click="patch({ items: modelValue.items.filter((_, at) => at !== index) })"><X :size="14" /></a-button>
      </div>
      <a-button size="small" type="text" class="mcp-add" @click="patch({ items: [...modelValue.items, ''] })"><Plus :size="14" />添加{{ field.label === '启动参数' ? '参数' : '一项' }}</a-button>
    </template>
    <template v-else-if="field.kind === 'pairs'">
      <div v-for="(row, index) in modelValue.pairs" :key="index" class="mcp-pair-row">
        <a-input :aria-invalid="Boolean(error)" :aria-describedby="`${id}-help ${id}-error`" :id="index === 0 ? id : undefined" :model-value="row.name" placeholder="名称" :aria-label="`${field.label}名称 ${index + 1}`" @update:model-value="pair(index, 'name', $event)" />
        <a-input :aria-invalid="Boolean(error)" :aria-describedby="`${id}-help ${id}-error`" :model-value="row.value" placeholder="值" :aria-label="`${field.label}值 ${index + 1}`" @update:model-value="pair(index, 'value', $event)" />
        <a-button type="text" :aria-label="`移除${field.label} ${index + 1}`" @click="patch({ pairs: modelValue.pairs.filter((_, at) => at !== index) })"><X :size="14" /></a-button>
      </div>
      <a-button size="small" type="text" class="mcp-add" @click="patch({ pairs: [...modelValue.pairs, { name: '', value: '' }] })"><Plus :size="14" />添加一行</a-button>
    </template>
    <a-textarea :aria-invalid="Boolean(error)" v-else-if="field.kind === 'json'" :id="id" :model-value="modelValue.text" :auto-size="{ minRows: 3, maxRows: 12 }" :aria-describedby="`${id}-help ${id}-error`" @update:model-value="patch({ text: $event })" />
    <a-input :aria-invalid="Boolean(error)" :aria-describedby="`${id}-help ${id}-error`" v-else :id="id" :model-value="modelValue.text" :placeholder="field.placeholder ?? '可选，按服务说明填写'" :aria-required="required" @update:model-value="patch({ text: $event })" />
    <p v-if="error" :id="`${id}-error`" class="mcp-field-error" role="alert">{{ error }}</p>
  </div>
</template>
