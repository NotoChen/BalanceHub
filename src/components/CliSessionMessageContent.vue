<script setup lang="ts">
import AgentMarkdownContent from "./agent-workspace/AgentMarkdownContent.vue";
import SessionHighlight from "./agent-workspace/SessionHighlight.vue";
withDefaults(
  defineProps<{ content: string; query: string; plain?: boolean }>(),
  { plain: false },
);
</script>
<template>
  <pre
    v-if="plain"
    class="session-reader-raw"
  ><code><SessionHighlight :text="content" :terms="query.trim().split(/\s+/)" /></code></pre>
  <AgentMarkdownContent
    v-else
    :text="content"
    :query="query"
    preserve-frontmatter
  />
</template>
