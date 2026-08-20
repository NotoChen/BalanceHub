<script setup lang="ts">
import { computed } from "vue";
import { Message } from "@arco-design/web-vue";
import { IconCopy } from "@arco-design/web-vue/es/icon";
import { copyText } from "../composables/useClipboard";

const props = defineProps<{
  content: string;
  query: string;
}>();

interface TextBlock {
  type: "text";
  key: string;
  content: string;
}

interface CodeBlock {
  type: "code";
  key: string;
  content: string;
  language: string;
}

type ContentBlock = TextBlock | CodeBlock;

const blocks = computed(() => parseContent(props.content));

function parseContent(content: string): ContentBlock[] {
  const normalized = content.replace(/\r\n?/g, "\n");
  const blocks: ContentBlock[] = [];
  const fence = /^```([^\n`]*)\n([\s\S]*?)^```[ \t]*$/gm;
  let cursor = 0;
  let match: RegExpExecArray | null;
  while ((match = fence.exec(normalized)) !== null) {
    pushTextBlocks(blocks, normalized.slice(cursor, match.index));
    blocks.push({
      type: "code",
      key: `code-${match.index}`,
      language: match[1].trim(),
      content: match[2].replace(/\n$/, ""),
    });
    cursor = match.index + match[0].length;
  }
  pushTextBlocks(blocks, normalized.slice(cursor));
  return blocks.length > 0
    ? blocks
    : [{ type: "text", key: "text-0", content: normalized }];
}

function pushTextBlocks(blocks: ContentBlock[], content: string) {
  const trimmed = content.replace(/^\n+|\n+$/g, "");
  if (!trimmed) return;
  let offset = 0;
  for (const paragraph of trimmed.split(/\n{2,}/)) {
    if (!paragraph) continue;
    blocks.push({ type: "text", key: `text-${blocks.length}-${offset}`, content: paragraph });
    offset += paragraph.length;
  }
}

function highlightedSegments(value: string) {
  const query = props.query.trim();
  if (!query) return [{ text: value, matched: false }];
  const pattern = new RegExp(`(${escapeRegExp(query)})`, "gi");
  return value
    .split(pattern)
    .filter(Boolean)
    .map((text) => ({
      text,
      matched: text.toLocaleLowerCase() === query.toLocaleLowerCase(),
    }));
}

function escapeRegExp(value: string) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

async function copyCode(content: string) {
  try {
    await copyText(content);
    Message.success("已复制代码块");
  } catch (error) {
    Message.error(error instanceof Error ? error.message : String(error));
  }
}
</script>

<template>
  <div class="cli-session-rich-content">
    <template v-for="block in blocks" :key="block.key">
      <p v-if="block.type === 'text'">
        <template
          v-for="(segment, segmentIndex) in highlightedSegments(block.content)"
          :key="segmentIndex"
        >
          <mark v-if="segment.matched">{{ segment.text }}</mark>
          <template v-else>{{ segment.text }}</template>
        </template>
      </p>
      <section v-else class="cli-session-code-block">
        <header>
          <span>{{ block.language || "代码" }}</span>
          <button
            type="button"
            title="复制代码块"
            aria-label="复制代码块"
            @click="copyCode(block.content)"
          >
            <icon-copy />
            <span>复制</span>
          </button>
        </header>
        <pre><code><template v-for="(segment, segmentIndex) in highlightedSegments(block.content)" :key="segmentIndex"><mark v-if="segment.matched">{{ segment.text }}</mark><template v-else>{{ segment.text }}</template></template></code></pre>
      </section>
    </template>
  </div>
</template>
