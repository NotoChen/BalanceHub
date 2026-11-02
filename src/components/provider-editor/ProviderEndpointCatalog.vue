<script setup lang="ts">
import { computed, ref } from "vue";
import { IconDelete, IconPlus } from "@arco-design/web-vue/es/icon";
import type {
  ProviderEndpointDescriptor,
  ProviderEndpointOverride,
  ProviderEndpointQuery,
  ProviderHeader,
  ProviderInput,
} from "../../stores/providers";

/// 接口目录：把协议会发出的请求按组列出来，默认值只读，改写项随卡片保存。
/// 这里的展示顺序和默认值都来自 Rust 的协议目录，不在前端另写一份。
const props = defineProps<{
  draft: ProviderInput;
  endpoints: ProviderEndpointDescriptor[];
  limit: number;
}>();

const expanded = ref("");
// 旧草稿或外部构造的输入可能没有这个字段；缺省视为没有改写。
if (!Array.isArray(props.draft.request.endpoints)) {
  props.draft.request.endpoints = [];
}
const groupOrder = [
  "account",
  "checkIn",
  "announcements",
  "apiKeys",
  "usage",
  "models",
  "links",
];

interface EndpointGroup {
  key: string;
  label: string;
  endpoints: ProviderEndpointDescriptor[];
  overridden: number;
}

const groups = computed<EndpointGroup[]>(() => {
  const map = new Map<string, EndpointGroup>();
  for (const endpoint of props.endpoints) {
    const group = map.get(endpoint.group) ?? {
      key: endpoint.group,
      label: endpoint.groupLabel,
      endpoints: [],
      overridden: 0,
    };
    group.endpoints.push(endpoint);
    if (overrideOf(endpoint.id)) group.overridden += 1;
    map.set(endpoint.group, group);
  }
  return [...map.values()].sort(
    (left, right) =>
      groupOrder.indexOf(left.key) - groupOrder.indexOf(right.key),
  );
});

const overriddenCount = computed(
  () => endpointOverrides().filter((over) => !isEmptyOverride(over)).length,
);
const canAdd = computed(
  () => endpointOverrides().length < props.limit,
);

function endpointOverrides(): ProviderEndpointOverride[] {
  return props.draft.request.endpoints;
}

function overrideOf(operation: string): ProviderEndpointOverride | undefined {
  return endpointOverrides().find((over) => over.operation === operation);
}

function isEmptyOverride(over: ProviderEndpointOverride) {
  return !over.path.trim() && !over.query.length && !over.headers.length;
}

function startOverride(endpoint: ProviderEndpointDescriptor) {
  let over = overrideOf(endpoint.id);
  if (!over) {
    if (!canAdd.value) return;
    over = { operation: endpoint.id, path: "", query: [], headers: [] };
    props.draft.request.endpoints.push(over);
  }
  expanded.value = expanded.value === endpoint.id ? "" : endpoint.id;
  // 只点了「改写」又没有填写时，收起后不留空条目。
  if (expanded.value === "" && isEmptyOverride(over)) {
    removeOverride(endpoint.id);
  }
}

function removeOverride(operation: string) {
  props.draft.request.endpoints = props.draft.request.endpoints.filter(
    (over) => over.operation !== operation,
  );
  if (expanded.value === operation) expanded.value = "";
}

function pathValue(operation: string) {
  return overrideOf(operation)?.path ?? "";
}

function setPath(operation: string, value: string) {
  const over = overrideOf(operation);
  if (over) over.path = value;
}

function queryFields(operation: string): ProviderEndpointQuery[] {
  return overrideOf(operation)?.query ?? [];
}

function headerFields(operation: string): ProviderHeader[] {
  return overrideOf(operation)?.headers ?? [];
}

function addQuery(operation: string) {
  overrideOf(operation)?.query.push({ name: "", value: "" });
}

function removeQuery(operation: string, index: number) {
  overrideOf(operation)?.query.splice(index, 1);
}

function addHeader(operation: string) {
  overrideOf(operation)?.headers.push({ name: "", value: "" });
}

function removeHeader(operation: string, index: number) {
  overrideOf(operation)?.headers.splice(index, 1);
}

function displayPath(endpoint: ProviderEndpointDescriptor) {
  return overrideOf(endpoint.id)?.path.trim() || endpoint.path;
}

function showsDefaultQuery(endpoint: ProviderEndpointDescriptor) {
  return Boolean(endpoint.query) && !overrideOf(endpoint.id)?.path.trim();
}
</script>

<template>
  <div class="provider-endpoint-catalog">
    <p class="provider-endpoint-intro">
      下面是 BalanceHub 对这个协议会发出的全部请求。空白表示沿用默认约定；
      改写只作用于本卡片，保存后从下一次请求开始生效。
      <span v-if="overriddenCount" class="provider-backup-url-count">
        已改写 {{ overriddenCount }} 个接口
      </span>
    </p>
    <details
      v-for="group in groups"
      :key="group.key"
      class="provider-endpoint-group"
    >
      <summary>
        <span>{{ group.label }}</span>
        <span class="provider-backup-url-count">
          {{ group.endpoints.length }} 个接口
          <template v-if="group.overridden">· {{ group.overridden }} 项改写</template>
        </span>
      </summary>
      <div class="provider-endpoint-list">
        <article
          v-for="endpoint in group.endpoints"
          :key="endpoint.id"
          class="provider-endpoint-item"
          :class="{ 'is-overridden': overrideOf(endpoint.id) }"
        >
          <header class="provider-endpoint-head">
            <span class="provider-endpoint-method">
              {{ endpoint.method ?? "链接" }}
            </span>
            <span class="provider-endpoint-label">{{ endpoint.label }}</span>
            <code class="provider-endpoint-path">
              {{ displayPath(endpoint)
              }}<template v-if="showsDefaultQuery(endpoint)"
                >?{{ endpoint.query }}</template
              >
            </code>
            <a-button
              type="text"
              size="small"
              :disabled="!overrideOf(endpoint.id) && !canAdd"
              @click="startOverride(endpoint)"
            >
              {{
                expanded === endpoint.id
                  ? "收起"
                  : overrideOf(endpoint.id)
                    ? "编辑改写"
                    : "改写"
              }}
            </a-button>
          </header>
          <p class="provider-endpoint-description">{{ endpoint.description }}</p>
          <dl class="provider-endpoint-defaults">
            <template v-if="endpoint.method">
              <dt>默认请求</dt>
              <dd>
                <code>
                  {{ endpoint.method }} {{ endpoint.path
                  }}<template v-if="endpoint.query">?{{ endpoint.query }}</template>
                </code>
              </dd>
              <dt v-if="endpoint.altPaths.length">备用路径</dt>
              <dd v-if="endpoint.altPaths.length">
                <code>{{ endpoint.altPaths.join(" / ") }}</code>
              </dd>
            </template>
            <dt>认证</dt>
            <dd>{{ endpoint.authLabel }}</dd>
            <dt>请求头</dt>
            <dd>
              <span v-if="endpoint.headers.length" class="provider-endpoint-headers">
                <span
                  v-for="header in endpoint.headers"
                  :key="header.name"
                  class="provider-endpoint-header"
                >
                  <code>{{ header.name }}: {{ header.value }}</code>
                </span>
              </span>
              <span v-else>无</span>
            </dd>
            <template v-if="endpoint.placeholders.length">
              <dt>占位符</dt>
              <dd>
                <code>{{
                  endpoint.placeholders.map((name) => "{" + name + "}").join(" ")
                }}</code>
              </dd>
            </template>
            <template v-if="endpoint.jsonBody">
              <dt>请求体</dt>
              <dd>JSON 对象；字段由对应操作写入。</dd>
            </template>
            <template v-if="!endpoint.method">
              <dt>说明</dt>
              <dd>复制邀请链接时直接使用这个模板，<code>{code}</code> 会替换为邀请码。</dd>
            </template>
          </dl>
          <div
            v-if="expanded === endpoint.id && overrideOf(endpoint.id)"
            class="provider-endpoint-editor"
          >
            <a-form-item
              class="provider-field"
              :label="endpoint.method ? '路径' : '链接模板'"
            >
              <a-input
                :model-value="pathValue(endpoint.id)"
                :placeholder="`留空沿用 ${endpoint.path}`"
                allow-clear
                @update:model-value="(value) => setPath(endpoint.id, String(value ?? ''))"
              />
              <template #extra>
                <template v-if="endpoint.method">
                  只填地址后半段，必须以 / 开头；查询参数写在下面，不要放进路径。
                </template>
                <template v-else>
                  以 / 开头，保留 <code>{code}</code> 占位符；可以改写注册路径和参数名。
                </template>
              </template>
            </a-form-item>
            <div v-if="endpoint.method" class="provider-endpoint-subsection">
              <div class="provider-form-subsection-header">
                <span>查询参数</span>
                <a-button
                  type="text"
                  size="small"
                  @click="addQuery(endpoint.id)"
                >
                  <template #icon><IconPlus /></template>添加参数
                </a-button>
              </div>
              <div
                v-if="queryFields(endpoint.id).length"
                class="provider-backup-url-list"
              >
                <div
                  v-for="(field, index) in queryFields(endpoint.id)"
                  :key="index"
                  class="provider-backup-url-row provider-header-row"
                >
                  <a-input
                    v-model="field.name"
                    :aria-label="`接口参数 ${index + 1} 名称`"
                    placeholder="size"
                    allow-clear
                  />
                  <a-input
                    v-model="field.value"
                    :aria-label="`接口参数 ${index + 1} 值`"
                    placeholder="值；留空删除同名默认参数"
                    allow-clear
                  />
                  <a-button
                    type="text"
                    status="danger"
                    :aria-label="`删除接口参数 ${index + 1}`"
                    @click="removeQuery(endpoint.id, index)"
                  >
                    <template #icon><IconDelete /></template>
                  </a-button>
                </div>
              </div>
              <p v-else class="provider-empty-note">
                未添加参数；默认参数保持原样。
              </p>
            </div>
            <div v-if="endpoint.method" class="provider-endpoint-subsection">
              <div class="provider-form-subsection-header">
                <span>请求头</span>
                <a-button
                  type="text"
                  size="small"
                  @click="addHeader(endpoint.id)"
                >
                  <template #icon><IconPlus /></template>添加请求头
                </a-button>
              </div>
              <div
                v-if="headerFields(endpoint.id).length"
                class="provider-backup-url-list"
              >
                <div
                  v-for="(header, index) in headerFields(endpoint.id)"
                  :key="index"
                  class="provider-backup-url-row provider-header-row"
                >
                  <a-input
                    v-model="header.name"
                    :aria-label="`接口请求头 ${index + 1} 名称`"
                    placeholder="X-Custom-Header"
                    allow-clear
                  />
                  <a-input
                    v-model="header.value"
                    :aria-label="`接口请求头 ${index + 1} 值`"
                    placeholder="值"
                    allow-clear
                  />
                  <a-button
                    type="text"
                    status="danger"
                    :aria-label="`删除接口请求头 ${index + 1}`"
                    @click="removeHeader(endpoint.id, index)"
                  >
                    <template #icon><IconDelete /></template>
                  </a-button>
                </div>
              </div>
              <p v-else class="provider-empty-note">
                未添加请求头；卡片级请求头和认证头保持不变。
              </p>
            </div>
            <a-button
              type="text"
              size="small"
              status="danger"
              @click="removeOverride(endpoint.id)"
            >
              恢复默认
            </a-button>
          </div>
        </article>
      </div>
    </details>
    <p v-if="!groups.length" class="provider-empty-note">
      当前协议没有可列出的接口。
    </p>
  </div>
</template>
