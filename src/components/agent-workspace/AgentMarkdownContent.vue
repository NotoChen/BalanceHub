<script setup lang="ts">
import { computed, onScopeDispose, ref, watch } from "vue";
import { openExternalLink } from "../../api/app";
import { withTimeout } from "../../utils/promise-timeout";
import { documentMarkdownToHtml } from "../../utils/markdown-html";
import "../../styles/modules/agent-resource-content.css";
import { sessionHighlight } from "../../utils/session-highlight";

const props = withDefaults(
  defineProps<{
    text: string;
    query?: string;
    preserveFrontmatter?: boolean;
  }>(),
  { query: "", preserveFrontmatter: false },
);
const linkError = ref("");
let requestId = 0;
const rendered = computed(() => {
  // Frontmatter remains untouched in the source editor.
  const body = props.preserveFrontmatter
    ? props.text
    : props.text.replace(
        /^\uFEFF?---\r?\n([\s\S]{0,16384}?)\r?\n---(?:\r?\n|$)/,
        (frontmatter, header: string) =>
          /^(?:name|description|title|version):/m.test(header)
            ? ""
            : frontmatter,
      );
  const safe = documentMarkdownToHtml(body);
  const terms = [
    ...new Set(props.query.trim().split(/\s+/).filter(Boolean)),
  ].slice(0, 8);
  if (!terms.length) return safe;
  const fragment = document.createElement("template");
  fragment.innerHTML = safe;
  const walker = document.createTreeWalker(
    fragment.content,
    NodeFilter.SHOW_TEXT,
  );
  const nodes: Text[] = [];
  while (walker.nextNode()) nodes.push(walker.currentNode as Text);
  for (const node of nodes) {
    const segments = sessionHighlight(node.textContent ?? "", terms);
    if (!segments.some((segment) => segment.matched)) continue;
    const replacement = document.createDocumentFragment();
    for (const segment of segments) {
      if (segment.matched) {
        const mark = document.createElement("mark");
        mark.textContent = segment.text;
        replacement.append(mark);
      } else replacement.append(document.createTextNode(segment.text));
    }
    node.replaceWith(replacement);
  }
  return fragment.innerHTML;
});
watch(
  () => props.text,
  () => {
    requestId += 1;
    linkError.value = "";
  },
);
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
    await withTimeout(openExternalLink(url), 10_000, "打开链接超时");
  } catch {
    if (request === requestId) linkError.value = "无法打开链接，请稍后重试";
  }
}
</script>

<template>
  <div class="agent-resource-reading">
    <article
      v-if="rendered.trim()"
      class="agent-resource-markdown"
      @click="openLink"
      @auxclick.prevent
      v-html="rendered"
    />
    <p v-else class="agent-workspace-note">此文件还没有正文。</p>
    <p v-if="linkError" class="agent-workspace-error" role="alert">
      {{ linkError }}
    </p>
  </div>
</template>
