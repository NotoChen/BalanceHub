<script setup lang="ts">
import { computed, ref } from "vue";
import { CheckCheck, Megaphone, RefreshCw, TriangleAlert } from "@lucide/vue";
import type {
  ProviderProtocolDescriptor,
  SiteAnnouncement,
  SiteAnnouncementSourceError,
} from "../stores/providers";
import {
  formatSiteAnnouncementDateTime,
  groupSiteAnnouncements,
  siteAnnouncementExplicitTitle,
  siteAnnouncementLevelKey,
  siteAnnouncementLevelLabel,
} from "../utils/site-announcements";
import SiteAnnouncementContent from "./SiteAnnouncementContent.vue";

const props = defineProps<{
  visible: boolean;
  loading: boolean;
  fatalError: string;
  announcements: SiteAnnouncement[];
  errors: SiteAnnouncementSourceError[];
  unreadCount: number;
  markingFingerprints: Set<string>;
  providerProtocols: ProviderProtocolDescriptor[];
  isRead: (item: SiteAnnouncement) => boolean;
}>();

const emit = defineEmits<{
  "update:visible": [visible: boolean];
  refresh: [];
  markRead: [item: SiteAnnouncement];
  markAllRead: [];
}>();

/// 按站点分组后，站点顺序跟随各自最新一条公告。
const groups = computed(() => groupSiteAnnouncements(props.announcements, props.isRead));

/// 站点很多时一条线性时间线要翻很久；面板上方直接按站点切换，选中即只看该站点。
/// 选中的站点刷新后消失了就自动退回「全部」，不需要额外的重置状态。
const activeProviderId = ref("");
const activeGroup = computed(
  () => groups.value.find((group) => group.providerId === activeProviderId.value) ?? null,
);
const visibleGroups = computed(() => (activeGroup.value ? [activeGroup.value] : groups.value));

/// 滚动由弹窗主体统一承担，换站后必须回到顶部，否则会停在上一个站点的位置，
/// 一进来就是别人的公告中段。
const contentRef = ref<HTMLElement | null>(null);

function selectSite(providerId: string) {
  activeProviderId.value = providerId;
  contentRef.value?.closest(".arco-modal-body")?.scrollTo({ top: 0 });
}

function protocolLabel(protocol: SiteAnnouncement["providerProtocol"]) {
  return props.providerProtocols.find((item) => item.kind === protocol)?.label ?? protocol;
}

function entryTitle(item: SiteAnnouncement) {
  return siteAnnouncementExplicitTitle(item);
}
</script>

<template>
  <a-modal
    :visible="visible"
    modal-class="surface-modal site-announcements-modal"
    :footer="false"
    :width="940"
    unmount-on-close
    @update:visible="emit('update:visible', $event)"
  >
    <template #title>
      <div class="surface-modal-title site-announcements-title">
        <span class="surface-modal-title-icon"><Megaphone :size="18" :stroke-width="1.8" /></span>
        <span class="surface-modal-title-copy">
          <strong>站点公告</strong>
        </span>
        <span class="surface-modal-title-meta" :class="{ ready: announcements.length > 0 }">
          {{ groups.length }} 个站点 · {{ announcements.length }} 条公告<span v-if="unreadCount > 0"> · {{ unreadCount }} 条未读</span>
        </span>
      </div>
    </template>

    <div ref="contentRef" class="site-announcements-content">
      <div class="site-announcements-head">
        <header class="site-announcements-toolbar">
          <div>
            <span>按站点分组，组内按发布时间排列</span>
            <i v-if="errors.length > 0">{{ errors.length }} 个站点读取失败</i>
          </div>
          <div class="site-announcements-toolbar-actions">
            <a-tooltip content="重新读取站点公告">
              <a-button
                class="site-announcements-refresh"
                shape="circle"
                :disabled="loading"
                aria-label="重新读取站点公告"
                @click="emit('refresh')"
              >
                <template #icon>
                  <RefreshCw
                    :class="{ 'site-announcements-spin': loading }"
                    :size="17"
                    :stroke-width="1.9"
                  />
                </template>
              </a-button>
            </a-tooltip>
            <a-button
              class="site-announcements-mark-read"
              size="small"
              :disabled="unreadCount === 0"
              @click="emit('markAllRead')"
            >
              <template #icon><CheckCheck :size="15" :stroke-width="1.9" /></template>
              全部已读
            </a-button>
          </div>
        </header>

        <!-- 站点一多，线性时间线要一直往下翻；这里直接按站点切换。 -->
        <nav
          v-if="groups.length > 1"
          class="site-announcement-switcher"
          role="tablist"
          aria-label="按站点查看公告"
        >
          <button
            type="button"
            role="tab"
            class="site-announcement-switcher-chip"
            :class="{ active: !activeGroup }"
            :aria-selected="!activeGroup"
            @click="selectSite('')"
          >
            <span>全部</span>
            <b>{{ announcements.length }}</b>
          </button>
          <button
            v-for="group in groups"
            :key="group.providerId"
            type="button"
            role="tab"
            class="site-announcement-switcher-chip"
            :class="{ active: activeGroup?.providerId === group.providerId }"
            :aria-selected="activeGroup?.providerId === group.providerId"
            @click="selectSite(group.providerId)"
          >
            <span>{{ group.providerName }}</span>
            <b>{{ group.items.length }}</b>
            <em v-if="group.unreadCount > 0">{{ group.unreadCount }}</em>
          </button>
        </nav>
      </div>

      <a-alert v-if="fatalError" type="error" :show-icon="true">
        {{ fatalError }}
      </a-alert>

      <div v-if="loading && announcements.length === 0" class="site-announcements-skeleton">
        <a-skeleton v-for="index in 4" :key="index" animation>
          <a-skeleton-line :rows="2" :widths="['62%', '88%']" />
        </a-skeleton>
      </div>

      <template v-else-if="announcements.length > 0">
        <div class="site-announcements-timeline">
          <section v-for="group in visibleGroups" :key="group.providerId" class="site-announcement-group">
            <header class="site-announcement-group-header">
              <span class="site-announcement-group-name">{{ group.providerName }}</span>
              <i>{{ protocolLabel(group.providerProtocol) }}</i>
              <span class="site-announcement-group-count">{{ group.items.length }} 条</span>
              <em v-if="group.unreadCount > 0">{{ group.unreadCount }} 条未读</em>
            </header>

            <!-- 同一站点有两种来源时左右分栏：站点总公告长期有效，通常只有一条，
                 控制台公告是一串更新，上下叠在一起就得先把前者翻过去才能看后者。 -->
            <div
              class="site-announcement-sections"
              :class="{ 'is-split': group.sections.length > 1 }"
            >
              <section
                v-for="section in group.sections"
                :key="section.source"
                class="site-announcement-section"
              >
                <!-- 分栏时每一栏都需要自己的标题，否则左右两边读不出哪边是什么。 -->
                <header v-if="group.sections.length > 1" class="site-announcement-section-header">
                  <span>{{ section.label }}</span>
                  <i>{{ section.items.length }} 条</i>
                  <em v-if="section.unreadCount > 0">{{ section.unreadCount }} 条未读</em>
                </header>

                <article
                  v-for="item in section.items"
                  :key="item.fingerprint"
                  class="site-announcement-entry"
                >
                  <span class="site-announcement-rail" aria-hidden="true">
                    <span
                      class="site-announcement-rail-dot"
                      :data-level="siteAnnouncementLevelKey(item.level)"
                    />
                  </span>
                  <div class="site-announcement-entry-main">
                    <header class="site-announcement-entry-head">
                      <span class="site-announcement-entry-meta">
                        <em
                          v-if="siteAnnouncementLevelLabel(item.level)"
                          class="site-announcement-level"
                          :data-level="item.level"
                        >
                          {{ siteAnnouncementLevelLabel(item.level) }}
                        </em>
                        <button
                          v-if="!isRead(item)"
                          type="button"
                          class="site-announcement-unread-tag"
                          aria-label="将这条公告标记为已读"
                          @click="emit('markRead', item)"
                        >
                          未读
                        </button>
                        <time>{{ formatSiteAnnouncementDateTime(item.updatedAt || item.publishedAt) }}</time>
                      </span>
                      <strong v-if="entryTitle(item)" class="site-announcement-entry-title">
                        {{ entryTitle(item) }}
                      </strong>
                    </header>
                    <div class="site-announcement-entry-body">
                      <SiteAnnouncementContent v-if="item.content.trim()" :text="item.content" />
                      <p v-else class="site-announcement-entry-empty">公告未提供正文</p>
                      <section v-if="item.extra.trim()" class="site-announcement-extra">
                        <strong>附加信息</strong>
                        <SiteAnnouncementContent :text="item.extra" />
                      </section>
                    </div>
                    <span
                      v-if="markingFingerprints.has(item.fingerprint)"
                      class="site-announcement-read-sync"
                    >
                      正在同步已读状态
                    </span>
                  </div>
                </article>
              </section>
            </div>
          </section>
        </div>

        <section v-if="errors.length > 0" class="site-announcement-errors">
          <header>
            <TriangleAlert :size="15" :stroke-width="1.9" />
            <strong>部分站点读取失败</strong>
          </header>
          <div v-for="error in errors" :key="`${error.providerId}:${error.message}`">
            <b>{{ error.providerName }}</b>
            <span>{{ error.message }}</span>
          </div>
        </section>
      </template>

      <a-empty
        v-else-if="!fatalError && errors.length === 0"
        description="当前没有可展示的站点公告"
      />

      <section v-if="announcements.length === 0 && errors.length > 0" class="site-announcement-errors is-standalone">
        <header>
          <TriangleAlert :size="15" :stroke-width="1.9" />
          <strong>站点公告读取失败</strong>
        </header>
        <div v-for="error in errors" :key="`${error.providerId}:${error.message}`">
          <b>{{ error.providerName }}</b>
          <span>{{ error.message }}</span>
        </div>
      </section>
    </div>
  </a-modal>
</template>
