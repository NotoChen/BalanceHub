<script setup lang="ts">
import { computed, onScopeDispose, ref, watch } from "vue";
import { marked } from "marked";
import DOMPurify from "dompurify";
import { openAgentResourceLink } from "../../api/agent-configuration";
import { withTimeout } from "../../utils/promise-timeout";
import "../../styles/modules/agent-resource-content.css";

const props = defineProps<{ text: string }>();
const linkError = ref("");
let requestId = 0;
const rendered = computed(() => {
  // Frontmatter remains untouched in the source editor.
  const body = props.text.replace(/^\uFEFF?---\r?\n([\s\S]{0,16384}?)\r?\n---(?:\r?\n|$)/,
    (frontmatter, header: string) => /^(?:name|description|title|version):/m.test(header) ? "" : frontmatter);
  return DOMPurify.sanitize(marked.parse(body, { async: false, gfm: true }), {
    ALLOWED_TAGS: ["p", "br", "hr", "h1", "h2", "h3", "h4", "h5", "h6", "strong", "em", "del", "blockquote", "ul", "ol", "li", "pre", "code", "table", "thead", "tbody", "tr", "th", "td", "a", "details", "summary", "kbd"],
    ALLOWED_ATTR: ["href", "title", "start", "colspan", "rowspan"],
    ALLOWED_URI_REGEXP: /^https?:\/\//i,
  });
});
watch(() => props.text, () => { requestId += 1; linkError.value = ""; });
onScopeDispose(() => { requestId += 1; });
async function openLink(event: MouseEvent) {
  const link = event.target instanceof Element ? event.target.closest("a") : null;
  if (!link) return;
  event.preventDefault();
  const url = link.getAttribute("href");
  if (!url || !/^https?:\/\//i.test(url)) return;
  const request = ++requestId;
  linkError.value = "";
  try { await withTimeout(openAgentResourceLink(url), 10_000, "打开链接超时"); }
  catch { if (request === requestId) linkError.value = "无法打开链接，请稍后重试"; }
}
</script>

<template>
  <div class="agent-resource-reading">
    <article v-if="rendered.trim()" class="agent-resource-markdown" @click="openLink" @auxclick.prevent v-html="rendered" />
    <p v-else class="agent-workspace-note">此文件还没有正文。</p>
    <p v-if="linkError" class="agent-workspace-error" role="alert">{{ linkError }}</p>
  </div>
</template>
