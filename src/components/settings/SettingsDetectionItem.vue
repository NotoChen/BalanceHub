<script setup lang="ts">
defineProps<{
  state: "checking" | "ok" | "error";
  name: string;
  detail: string;
}>();
</script>

<template>
  <article class="settings-detection-item" :class="`is-${state}`">
    <span class="settings-detection-mark">
      <slot name="icon" />
    </span>
    <div class="settings-detection-copy">
      <strong>{{ name }}</strong>
      <span :title="detail">{{ detail }}</span>
    </div>
    <i class="settings-detection-state" aria-hidden="true" />
  </article>
</template>

<style scoped>
.settings-detection-item {
  --detection-accent: rgb(var(--arcoblue-6));
  --detection-highlight: rgba(255, 255, 255, 0.82);
  --detection-shadow: rgba(29, 33, 41, 0.055);
  position: relative;
  display: flex;
  min-width: 0;
  min-height: 108px;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  border: 1px solid var(--glass-border);
  border-radius: 8px;
  background: var(--glass-card);
  box-shadow:
    inset 0 1px 0 var(--detection-highlight),
    0 4px 12px var(--detection-shadow);
  padding: 12px;
  text-align: center;
}

.settings-detection-item.is-ok {
  --detection-accent: rgb(var(--green-6));
}

.settings-detection-item.is-checking {
  --detection-accent: rgb(var(--arcoblue-6));
}

.settings-detection-item.is-error {
  --detection-accent: rgb(var(--red-6));
}

.settings-detection-mark {
  display: grid;
  width: 32px;
  height: 32px;
  flex: 0 0 auto;
  place-items: center;
}

.settings-detection-copy {
  display: grid;
  width: 100%;
  min-width: 0;
  gap: 2px;
}

.settings-detection-copy > strong,
.settings-detection-copy > span {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.settings-detection-copy > strong {
  color: var(--color-text-1);
  font-size: 12px;
  font-weight: 700;
  line-height: 1.3;
}

.settings-detection-copy > span {
  color: var(--color-text-3);
  font-size: 10px;
  font-variant-numeric: tabular-nums;
  line-height: 1.4;
}

.settings-detection-state {
  position: absolute;
  top: 10px;
  right: 10px;
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: var(--detection-accent);
  box-shadow: 0 0 0 3px var(--glass-card-muted);
}

:global(:root.theme-dark) .settings-detection-item {
  --detection-highlight: rgba(255, 255, 255, 0.065);
  --detection-shadow: rgba(0, 0, 0, 0.2);
}
</style>
