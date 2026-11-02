<script setup lang="ts">
import { computed, ref } from "vue";
import { IconDelete, IconPlus } from "@arco-design/web-vue/es/icon";
import type {
  ProviderInput,
  ProviderModelOverride,
} from "../../stores/providers";
import ProviderModelRuleEditor from "./ProviderModelRuleEditor.vue";

type PriceField = keyof ProviderModelOverride["price"];

const props = defineProps<{
  draft: ProviderInput;
  availableModels: string[];
  /// 只有通用 OpenAI 兼容协议会读取自定义模型列表地址。
  showModelsUrl: boolean;
}>();

const showOverrides = ref(false);
const addTarget = ref("");
/// 刚添加的覆盖直接展开：否则点「添加」只多出一条折叠行，还要再点一次。
const justAdded = ref("");

function emptyOverride(): ProviderModelOverride {
  return {
    displayName: "",
    wireName: "",
    alias: "",
    contextWindow: null,
    outputLimit: null,
    price: { input: null, output: null, cacheRead: null, cacheWrite: null },
    reasoningLevels: [],
    sendsReasoning: null,
    hidden: false,
  };
}

const overrides = computed(() =>
  Object.entries(props.draft.modelConfig.overrides),
);

const addOptions = computed(() => {
  const known = new Set(Object.keys(props.draft.modelConfig.overrides));
  return [...new Set(props.availableModels.map((model) => model.trim()))]
    .filter((model) => model && !known.has(model))
    .sort((left, right) => left.localeCompare(right))
    .map((model) => ({ label: model, value: model }));
});

function addOverride() {
  const id = addTarget.value.trim();
  if (!id || props.draft.modelConfig.overrides[id]) return;
  props.draft.modelConfig.overrides[id] = emptyOverride();
  addTarget.value = "";
  justAdded.value = id;
}

function removeOverride(id: string) {
  delete props.draft.modelConfig.overrides[id];
  if (justAdded.value === id) justAdded.value = "";
}

function overlayPrice(over: ProviderModelOverride, field: PriceField) {
  return over.price[field];
}

function setPrice(
  over: ProviderModelOverride,
  field: PriceField,
  value: number | undefined,
) {
  over.price[field] = value ?? null;
}

function setSendsReasoning(over: ProviderModelOverride, value: unknown) {
  over.sendsReasoning = value === "unset" ? null : value === "true";
}

function sendsReasoningValue(over: ProviderModelOverride) {
  return over.sendsReasoning === null
    ? "unset"
    : over.sendsReasoning
      ? "true"
      : "false";
}

const sendsReasoningOptions = [
  { label: "未声明", value: "unset" },
  { label: "发送推理参数", value: "true" },
  { label: "不发送推理参数", value: "false" },
];

function modelOverrideBadges(over: ProviderModelOverride) {
  return [
    over.hidden ? "已隐藏" : "",
    over.wireName.trim() ? `上游 ${over.wireName.trim()}` : "",
  ].filter(Boolean);
}
</script>

<template>
  <section class="provider-form-section">
    <h3 class="provider-form-section-title">模型展示</h3>
    <div class="provider-form-section-body">
      <a-form-item v-if="showModelsUrl" class="provider-field" label="模型列表地址">
        <a-input
          v-model="draft.modelConfig.modelsUrl"
          placeholder="https://relay.example.com/v1/models（留空按协议默认）"
          allow-clear
        />
      </a-form-item>
      <a-form-item class="provider-field" label="只展示匹配的模型">
        <ProviderModelRuleEditor
          v-model="draft.modelConfig.include"
          :candidates="availableModels"
          placeholder="输入 glob（如 claude-*）或 re: 正则后回车；留空展示全部"
        />
        <template #extra>匹配忽略大小写；优先级低于隐藏规则。</template>
      </a-form-item>
      <a-form-item class="provider-field" label="隐藏匹配的模型">
        <ProviderModelRuleEditor
          v-model="draft.modelConfig.exclude"
          :candidates="availableModels"
          placeholder="输入 glob 或 re: 正则后回车"
        />
      </a-form-item>
      <a-form-item class="provider-field" label="models.dev 供应商">
        <a-input
          v-model="draft.modelConfig.catalog"
          placeholder="留空按模型 ID 自动匹配"
          allow-clear
        />
        <template #extra>借用该供应商的上下文、价格等资料补全缺失字段。</template>
      </a-form-item>

      <div class="provider-form-subsection-header">
        <span>模型覆盖</span>
        <span class="provider-backup-url-actions">
          <span v-if="overrides.length" class="provider-backup-url-count">
            {{ overrides.length }} 个
          </span>
          <a-button type="text" size="small" @click="showOverrides = !showOverrides">
            <template #icon><IconPlus /></template>
            {{ showOverrides ? "收起" : "添加覆盖" }}
          </a-button>
        </span>
      </div>
      <div v-if="showOverrides" class="provider-model-override-add">
        <a-select
          v-model="addTarget"
          :options="addOptions"
          allow-search
          allow-create
          placeholder="选择或输入模型 ID"
          aria-label="添加模型覆盖"
          @press-enter="addOverride"
        />
        <a-button
          type="primary"
          :disabled="!addTarget.trim() || Boolean(draft.modelConfig.overrides[addTarget.trim()])"
          @click="addOverride"
        >
          添加
        </a-button>
      </div>
      <div v-if="overrides.length" class="provider-model-overrides">
        <details
          v-for="[id, over] in overrides"
          :key="id"
          class="provider-model-override"
          :open="id === justAdded"
        >
          <summary>
            <code>{{ id }}</code>
            <span
              v-for="badge in modelOverrideBadges(over)"
              :key="badge"
              class="provider-model-override-badge"
              >{{ badge }}</span
            >
            <a-button
              type="text"
              size="small"
              status="danger"
              :aria-label="`删除模型覆盖 ${id}`"
              @click.prevent.stop="removeOverride(id)"
            >
              <template #icon><IconDelete /></template>
            </a-button>
          </summary>
          <div class="provider-field-grid provider-model-override-fields">
            <p class="provider-model-override-group">身份</p>
            <a-form-item class="provider-field" label="显示名称">
              <a-input
                v-model="over.displayName"
                placeholder="选择器里显示的名称"
                allow-clear
              />
            </a-form-item>
            <a-form-item class="provider-field" label="实际上游模型 ID">
              <a-input
                v-model="over.wireName"
                placeholder="留空与模型 ID 相同"
                allow-clear
              />
            </a-form-item>
            <a-form-item class="provider-field" label="用量归并到">
              <a-input
                v-model="over.alias"
                placeholder="统计时归并到的模型 ID"
                allow-clear
              />
            </a-form-item>
            <p class="provider-model-override-group">元数据</p>
            <a-form-item class="provider-field" label="上下文窗口">
              <a-input-number
                :model-value="over.contextWindow ?? undefined"
                :min="1"
                :precision="0"
                placeholder="tokens"
                @update:model-value="(value) => (over.contextWindow = value ?? null)"
              />
            </a-form-item>
            <a-form-item class="provider-field" label="输出上限">
              <a-input-number
                :model-value="over.outputLimit ?? undefined"
                :min="1"
                :precision="0"
                placeholder="tokens"
                @update:model-value="(value) => (over.outputLimit = value ?? null)"
              />
            </a-form-item>
            <a-form-item class="provider-field" label="推理档位">
              <a-input-tag
                v-model="over.reasoningLevels"
                placeholder="如 low / high 后回车；留空不声明"
                allow-clear
              />
            </a-form-item>
            <a-form-item class="provider-field" label="推理参数">
              <a-select
                :model-value="sendsReasoningValue(over)"
                :options="sendsReasoningOptions"
                @update:model-value="(value) => setSendsReasoning(over, value)"
              />
            </a-form-item>
            <a-form-item class="provider-field provider-field-wide" label="价格（USD / 百万 tokens）">
              <div class="provider-field-grid provider-model-price-grid">
                <a-input-number
                  :model-value="overlayPrice(over, 'input') ?? undefined"
                  :min="0"
                  :precision="6"
                  placeholder="输入"
                  aria-label="输入价格"
                  @update:model-value="(value) => setPrice(over, 'input', value)"
                />
                <a-input-number
                  :model-value="overlayPrice(over, 'output') ?? undefined"
                  :min="0"
                  :precision="6"
                  placeholder="输出"
                  aria-label="输出价格"
                  @update:model-value="(value) => setPrice(over, 'output', value)"
                />
                <a-input-number
                  :model-value="overlayPrice(over, 'cacheRead') ?? undefined"
                  :min="0"
                  :precision="6"
                  placeholder="缓存读取"
                  aria-label="缓存读取价格"
                  @update:model-value="(value) => setPrice(over, 'cacheRead', value)"
                />
                <a-input-number
                  :model-value="overlayPrice(over, 'cacheWrite') ?? undefined"
                  :min="0"
                  :precision="6"
                  placeholder="缓存写入"
                  aria-label="缓存写入价格"
                  @update:model-value="(value) => setPrice(over, 'cacheWrite', value)"
                />
              </div>
            </a-form-item>
            <a-checkbox v-model="over.hidden" class="provider-field-wide">
              从模型选择器隐藏
            </a-checkbox>
          </div>
        </details>
      </div>
      <p v-else class="provider-empty-note">
        覆盖会优先于 models.dev 目录；未填写的字段仍由目录补全。
      </p>
    </div>
  </section>
</template>
