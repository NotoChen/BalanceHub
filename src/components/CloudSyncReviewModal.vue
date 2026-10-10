<script setup lang="ts">
import { computed, inject, ref, watch } from "vue";
import { Cloud, ArrowDown, ArrowUp, GitCompareArrows } from "@lucide/vue";
import { CLOUD_SYNC_CONTEXT } from "../composables/useCloudSync";
import type { CloudSyncChange, CloudSyncResolution } from "../api/cloud-sync";
import ContentDiff from "./ContentDiff.vue";

const sync = inject(CLOUD_SYNC_CONTEXT);
const review = computed(() => sync?.state.value?.status.review);
const selectedKey = ref("");
const fileIndex = ref(0);
const choices = ref<Record<string, "local" | "remote">>({});
const selected = computed(() =>
  review.value?.changes.find((change) => change.key === selectedKey.value),
);
const conflicts = computed(
  () => review.value?.changes.filter((change) => change.conflict) ?? [],
);
const unresolved = computed(
  () => conflicts.value.filter((change) => !choices.value[change.key]).length,
);
const files = computed(() => sync?.comparison.value?.files ?? []);
const file = computed(() => files.value[fileIndex.value]);
watch(files, (items) => {
  const source = items.findIndex((item) => item.path === "SKILL.md");
  const content = items.findIndex((item) => item.path !== "配置");
  fileIndex.value = source >= 0 ? source : Math.max(0, content);
});
watch(
  () => review.value?.id,
  () => {
    choices.value = {};
    selectedKey.value =
      review.value?.changes.find((change) => change.conflict)?.key ||
      review.value?.changes[0]?.key ||
      "";
  },
  { immediate: true },
);
watch(
  [selectedKey, () => sync?.reviewVisible.value, () => review.value?.id],
  ([key, visible]) => {
    fileIndex.value = 0;
    if (visible && key) void sync?.compare(key);
  },
  { immediate: true },
);
function changeLabel(change: CloudSyncChange) {
  if (change.conflict) return "需要选择";
  if (change.upload) return change.localDeleted ? "从云端删除" : "上传到云端";
  return change.remoteDeleted ? "从本机删除" : "下载到本机";
}
function confirm() {
  const resolutions: CloudSyncResolution[] = conflicts.value.map((change) => ({
    key: change.key,
    side: choices.value[change.key],
  }));
  void sync?.confirm(resolutions);
}
</script>

<template>
  <a-modal
    v-if="sync"
    v-model:visible="sync.reviewVisible.value"
    width="min(1240px, calc(100vw - 40px))"
    :footer="false"
    modal-class="surface-modal cloud-sync-review-modal"
    unmount-on-close
  >
    <template #title
      ><span class="cloud-review-title"
        ><GitCompareArrows :size="20" />{{
          review?.initial ? "首次同步 · 核对差异" : "处理同步冲突"
        }}</span
      ></template
    >
    <div v-if="review" class="cloud-review">
      <div class="cloud-review-summary">
        <span
          >{{ review.changes.length }} 项变化<span v-if="conflicts.length"
            >，{{ conflicts.length }} 项需要选择</span
          ></span
        ><span v-if="review.remoteDevice" class="cloud-review-device"
          ><Cloud :size="14" />{{ review.remoteDevice }}</span
        >
      </div>
      <p class="cloud-review-note">
        {{
          review.initial
            ? "确认后合并两端配置；标记为删除的条目也会同步。"
            : "分别选择保留本机或云端版本，其他变化会自动合并。"
        }}
      </p>
      <div class="cloud-review-workspace">
        <nav class="cloud-review-list" aria-label="同步变化条目">
          <button
            v-for="change in review.changes"
            :key="change.key"
            type="button"
            :class="{ active: selectedKey === change.key }"
            :aria-current="selectedKey === change.key ? 'true' : undefined"
            @click="selectedKey = change.key"
          >
            <strong>{{ change.title }}</strong
            ><span
              >{{ change.category
              }}<i
                v-if="change.conflict"
                :class="{ resolved: choices[change.key] }"
                >{{ choices[change.key] ? "已选择" : "冲突" }}</i
              ><ArrowUp v-else-if="change.upload" :size="13" /><ArrowDown
                v-else
                :size="13" /></span
            ><small>{{
              change.conflict && choices[change.key]
                ? choices[change.key] === "local"
                  ? "保留本机"
                  : "保留云端"
                : changeLabel(change)
            }}</small>
          </button>
        </nav>
        <section class="cloud-review-content" aria-label="同步内容差异">
          <header v-if="selected" class="cloud-review-content-heading">
            <strong>{{ selected.title }}</strong
            ><span class="cloud-review-legend"><i>− 本机</i><b>+ 云端</b></span>
          </header>
          <div v-if="selected?.conflict" class="cloud-review-choice">
            <span>保留哪个版本</span>
            <a-radio-group
              v-model="choices[selected.key]"
              type="button"
              :aria-label="selected.title + ' 的冲突处理'"
            >
              <a-radio value="local">{{
                selected.localDeleted ? "按本机删除" : "保留本机"
              }}</a-radio
              ><a-radio value="remote">{{
                selected.remoteDeleted ? "按云端删除" : "保留云端"
              }}</a-radio>
            </a-radio-group>
          </div>
          <div v-if="sync.comparing.value" class="cloud-review-empty">
            <a-spin /><span>正在读取差异</span>
          </div>
          <a-alert v-else-if="sync.compareError.value" type="error"
            >{{ sync.compareError.value
            }}<a-button size="small" @click="sync.compare(selectedKey)"
              >重试</a-button
            ></a-alert
          >
          <template v-else-if="file">
            <div
              v-if="files.length > 1"
              class="cloud-review-files"
              aria-label="资源文件"
            >
              <button
                v-for="(item, index) in files"
                :key="item.path"
                type="button"
                :class="{ active: fileIndex === index }"
                @click="fileIndex = index"
              >
                {{ item.path }}
              </button>
            </div>
            <p v-if="file.binary" class="cloud-review-note">
              二进制文件显示大小与内容校验值，选择版本会同步完整文件。
            </p>
            <ContentDiff
              :original-text="file.localText"
              :modified-text="file.remoteText"
            />
          </template>
          <div v-else class="cloud-review-empty">选择左侧条目查看 diff</div>
        </section>
      </div>
      <a-alert v-if="sync.error.value" type="error">{{
        sync.error.value
      }}</a-alert>
      <footer class="cloud-review-footer">
        <span>{{
          unresolved
            ? `还有 ${unresolved} 项冲突需要选择`
            : "差异已就绪，可以同步"
        }}</span
        ><a-button @click="sync.reviewVisible.value = false">稍后处理</a-button
        ><a-button
          type="primary"
          :disabled="unresolved > 0 || Boolean(sync.pending.value)"
          @click="confirm"
          >确认同步</a-button
        >
      </footer>
    </div>
  </a-modal>
</template>

<style scoped>
.cloud-review-title {
  display: inline-flex;
  align-items: center;
  gap: 9px;
}
.cloud-review {
  display: flex;
  flex-direction: column;
  gap: 14px;
  min-height: 0;
  height: min(720px, calc(100vh - 170px));
  color: var(--color-text-1);
}
.cloud-review-summary {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  font-size: 13px;
}
.cloud-review-device {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  color: var(--color-text-3);
}
.cloud-review-note {
  margin: 0;
  color: var(--color-text-3);
  font-size: 12px;
  line-height: 1.7;
}
.cloud-review-workspace {
  display: grid;
  grid-template-columns: 230px minmax(0, 1fr);
  flex: 1;
  min-height: 230px;
  overflow: hidden;
  border: 1px solid var(--color-border-2);
  border-radius: 9px;
}
.cloud-review-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
  overflow: auto;
  min-width: 0;
  padding: 10px;
  background: var(--color-fill-1);
  border-right: 1px solid var(--color-border-2);
}
.cloud-review-list > button {
  display: grid;
  flex-shrink: 0;
  gap: 7px;
  padding: 12px;
  border: 1px solid transparent;
  border-radius: 7px;
  text-align: left;
  color: var(--color-text-1);
  background: transparent;
  cursor: pointer;
}
.cloud-review-list > button:hover {
  background: var(--color-fill-2);
}
.cloud-review-list > button.active {
  background: rgb(var(--primary-1));
  border-color: rgb(var(--primary-3));
}
.cloud-review-list strong {
  font-size: 13px;
  font-weight: 500;
  overflow-wrap: anywhere;
}
.cloud-review-list span {
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-size: 11px;
  color: var(--color-text-3);
}
.cloud-review-list small {
  font-size: 11px;
  color: var(--color-text-3);
}
.cloud-review-list i {
  color: rgb(var(--orange-6));
  font-style: normal;
}
.cloud-review-list i.resolved {
  color: rgb(var(--success-6));
}
.cloud-review-content {
  display: flex;
  flex-direction: column;
  gap: 14px;
  min-height: 0;
  overflow: auto;
  padding: 18px;
  min-width: 0;
}
.cloud-review-content-heading {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
  font-size: 14px;
}
.cloud-review-legend {
  display: inline-flex;
  gap: 14px;
  font-size: 12px;
  flex-shrink: 0;
}
.cloud-review-legend i {
  color: rgb(var(--danger-6));
  font-style: normal;
}
.cloud-review-legend b {
  color: rgb(var(--success-6));
  font-weight: 400;
}
.cloud-review-choice {
  display: flex;
  flex-wrap: wrap;
  justify-content: space-between;
  align-items: center;
  gap: 10px;
  border-radius: 6px;
  background: var(--color-fill-1);
  padding: 12px;
  font-size: 12px;
}
.cloud-review-files {
  display: flex;
  gap: 6px;
  overflow-x: auto;
  flex-shrink: 0;
}
.cloud-review-files button {
  border: 0;
  border-radius: 5px;
  padding: 6px 10px;
  background: var(--color-fill-1);
  color: var(--color-text-2);
  font-size: 12px;
  cursor: pointer;
  white-space: nowrap;
}
.cloud-review-files button.active {
  background: rgb(var(--primary-1));
  color: rgb(var(--primary-6));
}
.cloud-review-empty {
  display: flex;
  flex: 1;
  justify-content: center;
  align-items: center;
  gap: 10px;
  font-size: 13px;
  color: var(--color-text-3);
}
.cloud-review-footer {
  display: flex;
  gap: 10px;
  align-items: center;
  flex-shrink: 0;
}
.cloud-review-footer > span {
  margin-right: auto;
  color: var(--color-text-3);
  font-size: 12px;
}
.cloud-review-content :deep(.configuration-diff) {
  flex: 1;
  min-height: 120px;
}
.cloud-review-content :deep(.configuration-diff-scroll) {
  max-height: none;
}
@media (max-width: 820px) {
  .cloud-review-workspace {
    grid-template-columns: 180px minmax(0, 1fr);
  }
  .cloud-review-content {
    padding: 12px;
  }
}
</style>
