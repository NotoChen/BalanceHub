<script setup lang="ts">
import { computed, onScopeDispose, ref } from "vue";
import { openExternalLink } from "../api/app";
import { announcementMarkdownToHtml } from "../utils/markdown-html";

const props = defineProps<{ text: string }>();

const linkError = ref("");
let requestId = 0;

/// 站点公告正文按 NewAPI / Sub2API 的同一读法渲染：Markdown + 受限 HTML，
/// 图片保留。
const rendered = computed(() => announcementMarkdownToHtml(props.text));

onScopeDispose(() => {
  requestId += 1;
});

async function openLink(event: MouseEvent) {
  const link =
    event.target instanceof Element ? event.target.closest("a") : null;
  if (!link) return;
  event.preventDefault();
  const url = link.getAttribute("href");
  if (!url || !/^https?:\/\//i.test(url)) return;
  const request = ++requestId;
  linkError.value = "";
  try {
    await openExternalLink(url);
  } catch {
    if (request === requestId) linkError.value = "无法打开链接，请稍后重试";
  }
}
</script>

<template>
  <div class="site-announcement-markdown" @click="openLink" @auxclick.prevent>
    <article v-html="rendered" />
    <p v-if="linkError" class="site-announcement-link-error" role="alert">
      {{ linkError }}
    </p>
  </div>
</template>
