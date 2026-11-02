<script setup lang="ts">
import { computed } from "vue";
import { IconDelete, IconPlus, IconRefresh } from "@arco-design/web-vue/es/icon";
import type {
  ProviderConnectionTestResult,
  ProviderInput,
  ProviderProtocolDescriptor,
  ProviderQuotaWindowSpec,
} from "../../stores/providers";
import { providerProtocolDescriptor } from "../../utils/provider-protocol";
import ProviderHeaderListEditor from "./ProviderHeaderListEditor.vue";

const props = defineProps<{
  draft: ProviderInput;
  providerProtocols: ProviderProtocolDescriptor[];
  testingConnection: boolean;
  connectionTestResult: ProviderConnectionTestResult | null;
}>();

const emit = defineEmits<{
  "test-connection": [];
}>();

const capabilities = computed(
  () =>
    providerProtocolDescriptor(
      props.providerProtocols,
      props.draft.identity.protocol,
    )?.capabilities,
);

const supported = computed(() => capabilities.value?.quotaSpec ?? false);
/// 窗口上限来自协议能力，不在界面里另写一个数字。
const windowLimit = computed(() => capabilities.value?.maxQuotaWindows ?? 0);

const methodOptions = [
  { label: "GET", value: "get" },
  { label: "POST", value: "post" },
];

/// 与底部「测试连接」共用同一次读取：表达式写完后不必滚到窗口底部，
/// 就在这里看到余额接口实际取到了什么。草稿一改，上游会清空结果，
/// 这里不会把上一版表达式的结论当成当前配置的结论。
const quotaStep = computed(
  () =>
    props.connectionTestResult?.steps.find((step) => step.name.includes("余额"))
    ?? null,
);
const previewPending = computed(
  () => props.testingConnection && Boolean(props.draft.quotaSpec.url.trim()),
);

const WINDOW_PRESETS = ["5 小时", "每日", "每周", "每月"];

function addWindow(label = "") {
  const windows: ProviderQuotaWindowSpec[] = props.draft.quotaSpec.windows;
  if (windows.length >= windowLimit.value) return;
  windows.push({ label, used: "", limit: "", resetsAt: "" });
}

function removeWindow(index: number) {
  props.draft.quotaSpec.windows.splice(index, 1);
}
</script>

<template>
  <section v-if="supported" class="provider-form-section">
    <h3 class="provider-form-section-title">
      余额接口
      <span class="provider-backup-url-actions">
        <a-button
          type="text"
          size="small"
          :loading="testingConnection"
          :disabled="!draft.quotaSpec.url.trim() || !draft.identity.baseUrl.trim()"
          @click="emit('test-connection')"
        >
          <template #icon><IconRefresh /></template>读取一次
        </a-button>
      </span>
    </h3>
    <div class="provider-form-section-body">
      <p class="provider-setting-summary">
        为没有内置额度接口的通用 API Key 中转站声明余额来源；表达式示例：
        <code>data.balance</code>、<code>sum(data.items.*.used)</code>、
        <code>data.plan.reset_at</code>。
      </p>
      <p v-if="previewPending" class="provider-quota-preview is-pending" role="status">
        正在读取余额接口…
      </p>
      <p
        v-else-if="quotaStep"
        class="provider-quota-preview"
        :class="quotaStep.ok ? 'is-ok' : 'is-error'"
        role="status"
      >
        {{ quotaStep.ok ? "读取成功" : "读取失败" }} · {{ quotaStep.message }}
      </p>
      <div class="provider-field-grid">
        <a-form-item class="provider-field provider-field-wide" label="接口地址">
          <a-input
            v-model="draft.quotaSpec.url"
            placeholder="https://relay.example.com/api/user/balance 或 /api/user/balance"
            allow-clear
          />
        </a-form-item>
        <a-form-item class="provider-field" label="请求方式">
          <a-select v-model="draft.quotaSpec.method" :options="methodOptions" />
        </a-form-item>
        <a-form-item class="provider-field" label="余额表达式">
          <a-input
            v-model="draft.quotaSpec.balance"
            placeholder="data.balance"
            allow-clear
          />
          <template #extra>取不到余额时，订阅窗口仍可单独展示。</template>
        </a-form-item>
        <a-form-item class="provider-field" label="总额表达式（可选）">
          <a-input
            v-model="draft.quotaSpec.total"
            placeholder="data.total"
            allow-clear
          />
          <template #extra>与余额一起用来计算已用比例。</template>
        </a-form-item>
        <a-form-item class="provider-field" label="换算单位">
          <a-input-number
            v-model="draft.quotaSpec.perUnit"
            :min="0"
            :precision="6"
            placeholder="1"
          />
          <template #extra>表达式数值 ÷ 换算单位 = 展示金额；0 表示不换算。</template>
        </a-form-item>
        <a-form-item class="provider-field" label="货币符号">
          <a-input
            v-model="draft.quotaSpec.currencySymbol"
            placeholder="跟随卡片"
            allow-clear
          />
        </a-form-item>
      </div>

      <details class="provider-form-disclosure">
        <summary>鉴权与请求头（可选）</summary>
        <div class="provider-form-disclosure-body">
          <a-form-item class="provider-field" label="访问令牌">
            <a-input-password
              v-model="draft.quotaSpec.token"
              placeholder="留空使用当前 API Key"
              allow-clear
            />
          </a-form-item>
          <a-form-item class="provider-field" label="接口请求头">
            <ProviderHeaderListEditor v-model="draft.quotaSpec.headers" />
            <template #extra>只附加到余额接口请求；名称或值为空的行不会发送。</template>
          </a-form-item>
        </div>
      </details>

      <div class="provider-form-subsection-header">
        <span>订阅窗口</span>
        <span class="provider-backup-url-actions">
          <span v-if="draft.quotaSpec.windows.length" class="provider-backup-url-count">
            {{ draft.quotaSpec.windows.length }} / {{ windowLimit }}
          </span>
          <a-button
            type="text"
            size="small"
            :disabled="draft.quotaSpec.windows.length >= windowLimit"
            @click="addWindow()"
          >
            <template #icon><IconPlus /></template>添加窗口
          </a-button>
        </span>
      </div>
      <div v-if="draft.quotaSpec.windows.length" class="provider-quota-windows">
        <div
          v-for="(window, index) in draft.quotaSpec.windows"
          :key="index"
          class="provider-quota-window"
        >
          <div class="provider-backup-url-row">
            <span class="provider-backup-url-index">{{ index + 1 }}</span>
            <a-input
              v-model="window.label"
              :aria-label="`窗口 ${index + 1} 名称`"
              placeholder="如：5 小时额度"
              allow-clear
            />
            <a-button
              type="text"
              status="danger"
              :aria-label="`删除窗口 ${index + 1}`"
              @click="removeWindow(index)"
            >
              <template #icon><IconDelete /></template>
            </a-button>
          </div>
          <div class="provider-field-grid provider-quota-window-fields">
            <a-input
              v-model="window.used"
              :aria-label="`窗口 ${index + 1} 已用表达式`"
              placeholder="已用：data.used"
              allow-clear
            />
            <a-input
              v-model="window.limit"
              :aria-label="`窗口 ${index + 1} 上限表达式`"
              placeholder="上限：data.limit"
              allow-clear
            />
            <a-input
              v-model="window.resetsAt"
              class="provider-field-wide"
              :aria-label="`窗口 ${index + 1} 重置时间表达式`"
              placeholder="重置时间（可选）：data.reset_at"
              allow-clear
            />
          </div>
        </div>
      </div>
      <template v-else>
        <p class="provider-empty-note">
          可声明 5 小时、每周等订阅窗口；至少填写已用或上限表达式。
        </p>
        <div class="provider-quota-presets">
          <a-button
            v-for="preset in WINDOW_PRESETS"
            :key="preset"
            size="mini"
            @click="addWindow(`${preset}额度`)"
          >{{ preset }}</a-button>
        </div>
      </template>
    </div>
  </section>
</template>
