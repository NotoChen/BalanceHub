<script setup lang="ts">
import { computed } from "vue";
import { IconDownload, IconRight } from "@arco-design/web-vue/es/icon";
import { parseReleaseNotes } from "../utils/release-notes";

const props = defineProps<{
  visible: boolean;
  currentVersion: string;
  version: string;
  releaseNotes: string;
  installing: boolean;
  downloadProgress: number | null;
  installStatus: string;
}>();

const emit = defineEmits<{
  dismiss: [];
  install: [];
}>();

const sections = computed(() => parseReleaseNotes(props.releaseNotes, props.version));
const progressPercent = computed(() =>
  props.downloadProgress === null ? 0 : Math.min(1, Math.max(0, props.downloadProgress / 100)),
);

function handleVisibleChange(visible: boolean) {
  if (!visible && !props.installing) {
    emit("dismiss");
  }
}
</script>

<template>
  <a-modal
    :visible="visible"
    width="min(600px, calc(100vw - 32px))"
    title-align="start"
    modal-class="app-update-modal"
    :closable="!installing"
    :mask-closable="!installing"
    :esc-to-close="!installing"
    unmount-on-close
    @update:visible="handleVisibleChange"
  >
    <template #title>
      <div class="app-update-title">
        <span class="app-update-title-icon" aria-hidden="true"><icon-download /></span>
        <span>BalanceHub {{ version }} 已可用</span>
      </div>
    </template>

    <div class="app-update-panel">
      <div class="app-update-version-summary" aria-label="版本变化">
        <div class="app-update-version-item">
          <span>当前版本</span>
          <strong>{{ currentVersion || "未知" }}</strong>
        </div>
        <icon-right class="app-update-version-arrow" aria-hidden="true" />
        <div class="app-update-version-item app-update-version-target">
          <span>新版本</span>
          <strong>{{ version }}</strong>
        </div>
      </div>

      <section class="app-update-release-notes">
        <h3>更新内容</h3>
        <div class="app-update-release-scroll" tabindex="0" aria-label="版本更新说明">
          <template v-if="sections.length > 0">
            <section
              v-for="(section, sectionIndex) in sections"
              :key="`${section.title}-${sectionIndex}`"
              class="app-update-release-section"
            >
              <h4 v-if="section.title">{{ section.title }}</h4>
              <p
                v-for="(paragraph, paragraphIndex) in section.paragraphs"
                :key="`paragraph-${paragraphIndex}`"
              >
                <component
                  :is="segment.kind === 'code' ? 'code' : segment.kind === 'strong' ? 'strong' : 'span'"
                  v-for="(segment, segmentIndex) in paragraph"
                  :key="segmentIndex"
                >
                  {{ segment.value }}
                </component>
              </p>
              <ul v-if="section.items.length > 0">
                <li v-for="(item, itemIndex) in section.items" :key="itemIndex">
                  <component
                    :is="segment.kind === 'code' ? 'code' : segment.kind === 'strong' ? 'strong' : 'span'"
                    v-for="(segment, segmentIndex) in item"
                    :key="segmentIndex"
                  >
                    {{ segment.value }}
                  </component>
                </li>
              </ul>
            </section>
          </template>
          <p v-else class="app-update-release-empty">本次更新未提供详细说明。</p>
        </div>
      </section>

      <div v-if="installing" class="app-update-progress" aria-live="polite">
        <div class="app-update-progress-label">
          <strong>{{ installStatus || "正在准备更新" }}</strong>
          <span v-if="downloadProgress !== null">{{ downloadProgress }}%</span>
        </div>
        <a-progress
          v-if="downloadProgress !== null"
          :percent="progressPercent"
          :show-text="false"
          size="small"
          animation
        />
      </div>
    </div>

    <template #footer>
      <div class="app-update-footer">
        <p>安装完成后应用将自动重启。</p>
        <div class="app-update-actions">
          <a-button :disabled="installing" @click="emit('dismiss')">稍后</a-button>
          <a-button type="primary" :loading="installing" @click="emit('install')">
            {{ installing ? "正在更新" : "安装并重启" }}
          </a-button>
        </div>
      </div>
    </template>
  </a-modal>
</template>
