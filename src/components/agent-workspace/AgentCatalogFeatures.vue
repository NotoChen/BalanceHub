<script setup lang="ts">
import { computed } from "vue";
import type { AgentCatalogAsset } from "../../stores/agent-catalog-types";
import { agentAssetProvisionLabels, agentAssetInstallationLabels } from "../../utils/agent-asset-provenance";

const props = defineProps<{ asset: AgentCatalogAsset }>();
const emit = defineEmits<{ select: [feature: string] }>();
const features = computed(() => {
  const asset = props.asset;
  const values = [
    ...(asset.ownership === "managed" ? [{ id: "ownership-managed", label: `共享库${asset.version === null ? "" : ` v${asset.version}`}` }] : []),
    ...asset.provenance.provisions.filter((value) => value === "agentBuiltIn" || value === "pluginProvided")
      .map((value) => ({ id: `provision-${value}`, label: agentAssetProvisionLabels[value] })),
    ...asset.provenance.installations.filter((value) => value !== "unknown")
      .map((value) => ({ id: `installation-${value}`, label: agentAssetInstallationLabels[value] })),
  ];
  if (asset.candidateIds.length) values.push({ id: "same-name", label: `另有 ${asset.candidateIds.length} 份同名资源` });
  if (asset.variants.length > 1) values.push({ id: "variants", label: `${asset.variants.length} 份配置存在差异` });
  if (!asset.bindings.length && !asset.unresolvedTargets.length && asset.application.observations.length
    && asset.application.observations.every((observation) => observation.state === "missing")) {
    values.push({ id: "unapplied", label: "尚未配置到 Agent" });
  }
  return values;
});
</script>

<template>
  <span v-if="features.length" class="agent-catalog-badges is-row">
    <a-tooltip v-for="feature in features" :key="feature.id" :content="feature.label" :trigger="['hover', 'focus']">
      <button type="button" class="agent-catalog-badge" :class="{ 'is-variant': feature.id === 'variants', 'is-library': feature.id === 'ownership-managed' }" :data-asset-feature="feature.id" :aria-label="feature.label" @click="emit('select', feature.id)"><span>{{ feature.label }}</span></button>
    </a-tooltip>
  </span>
</template>
