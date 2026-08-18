<script setup lang="ts">
import { computed } from "vue";
import { CheckCheck, Megaphone, RefreshCw, TriangleAlert } from "@lucide/vue";
import type {
  ProviderProtocolDescriptor,
  SiteAnnouncement,
  SiteAnnouncementSourceError,
} from "../stores/providers";
import {
  formatSiteAnnouncementDateTime,
  siteAnnouncementDisplayContent,
  siteAnnouncementDisplayTitle,
} from "../utils/site-announcements";

const props = defineProps<{
  visible: boolean;
  loading: boolean;
  fatalError: string;
  announcements: SiteAnnouncement[];
  errors: SiteAnnouncementSourceError[];
  selected: SiteAnnouncement | null;
  unreadCount: number;
  markingFingerprints: Set<string>;
  providerProtocols: ProviderProtocolDescriptor[];
  isRead: (item: SiteAnnouncement) => boolean;
}>();

const emit = defineEmits<{
  "update:visible": [visible: boolean];
  refresh: [];
  select: [item: SiteAnnouncement];
  markAllRead: [];
}>();

const selectedProtocolLabel = computed(() =>
  props.selected ? protocolLabel(props.selected.providerProtocol) : "",
);

function protocolLabel(protocol: SiteAnnouncement["providerProtocol"]) {
  return props.providerProtocols.find((item) => item.kind === protocol)?.label ?? protocol;
}
</script>

<template>
  <a-modal
    :visible="visible"
    modal-class="surface-modal site-announcements-modal"
    :footer="false"
    :width="920"
    unmount-on-close
    @update:visible="emit('update:visible', $event)"
  >
    <template #title>
      <div class="surface-modal-title site-announcements-title">
        <span class="surface-modal-title-icon"><Megaphone :size="18" :stroke-width="1.8" /></span>
        <span class="surface-modal-title-copy">
          <span>来自已启用中转站</span>
          <strong>站点公告</strong>
        </span>
        <span class="surface-modal-title-meta" :class="{ ready: announcements.length > 0 }">
          {{ announcements.length }} 条公告<span v-if="unreadCount > 0"> · {{ unreadCount }} 条未读</span>
        </span>
      </div>
    </template>

    <div class="site-announcements-content">
      <header class="site-announcements-toolbar">
        <div>
          <strong>{{ announcements.length }}</strong>
          <span>条站点公告</span>
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
            :disabled="loading || unreadCount === 0"
            @click="emit('markAllRead')"
          >
            <template #icon><CheckCheck :size="15" :stroke-width="1.9" /></template>
            全部已读
          </a-button>
        </div>
      </header>

      <a-alert v-if="fatalError" type="error" :show-icon="true">
        {{ fatalError }}
      </a-alert>

      <div v-if="loading && announcements.length === 0" class="site-announcements-skeleton">
        <a-skeleton v-for="index in 4" :key="index" animation>
          <a-skeleton-line :rows="2" :widths="['62%', '88%']" />
        </a-skeleton>
      </div>

      <div v-else-if="announcements.length > 0" class="site-announcements-layout">
        <nav class="site-announcements-list" aria-label="站点公告列表">
          <button
            v-for="item in announcements"
            :key="item.fingerprint"
            type="button"
            class="site-announcement-list-item"
            :class="{
              'is-selected': selected?.fingerprint === item.fingerprint,
              'is-unread': !isRead(item),
            }"
            @click="emit('select', item)"
          >
            <span class="site-announcement-unread-dot" aria-hidden="true" />
            <span class="site-announcement-list-copy">
              <small>{{ item.providerName }}</small>
              <strong>{{ siteAnnouncementDisplayTitle(item) }}</strong>
              <time>{{ formatSiteAnnouncementDateTime(item.updatedAt || item.publishedAt) }}</time>
            </span>
          </button>

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
        </nav>

        <article v-if="selected" class="site-announcement-detail">
          <header>
            <div class="site-announcement-source">
              <span>{{ selected.providerName }}</span>
              <i>{{ selectedProtocolLabel }}</i>
            </div>
            <h3>{{ siteAnnouncementDisplayTitle(selected) }}</h3>
            <div class="site-announcement-time-row">
              <time>{{ formatSiteAnnouncementDateTime(selected.updatedAt || selected.publishedAt) }}</time>
              <span
                v-if="markingFingerprints.has(selected.fingerprint)"
                class="site-announcement-read-sync"
              >
                正在同步已读状态
              </span>
            </div>
          </header>
          <div class="site-announcement-body" :class="{ 'is-empty': !selected.content.trim() }">
            {{ siteAnnouncementDisplayContent(selected.content) }}
          </div>
        </article>

        <div v-else class="site-announcement-detail-empty">
          <Megaphone :size="28" :stroke-width="1.5" aria-hidden="true" />
          <strong>选择一条公告</strong>
          <span>查看完整内容，并将该公告标记为已读。</span>
        </div>
      </div>

      <a-empty
        v-else
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
