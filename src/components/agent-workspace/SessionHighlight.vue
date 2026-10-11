<script setup lang="ts">
import { computed } from "vue";
import { sessionHighlight } from "../../utils/session-highlight";
const props = defineProps<{ text: string; terms: readonly string[] }>();
const segments = computed(() => sessionHighlight(props.text, props.terms));
</script>
<template>
  <template v-for="(segment, index) in segments" :key="index"
    ><mark v-if="segment.matched">{{ segment.text }}</mark
    ><template v-else>{{ segment.text }}</template></template
  >
</template>
